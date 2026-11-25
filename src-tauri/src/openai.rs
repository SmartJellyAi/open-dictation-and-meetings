//! Calls to OpenAI with the person's own key: writing (summaries,
//! preparation, tidying a dictation), transcribing a recording, and a web
//! search for the people on a meeting.
//!
//! Every function here blocks; call them from a thread or a blocking task,
//! never while holding the database lock. Errors come back as a plain
//! sentence a person can act on; the details go to the log.

use std::time::Duration;

const CHAT_URL: &str = "https://api.openai.com/v1/chat/completions";
const TRANSCRIBE_URL: &str = "https://api.openai.com/v1/audio/transcriptions";
const RESPONSES_URL: &str = "https://api.openai.com/v1/responses";

/// The model that writes meeting notes and preparation. Override with the
/// `ODM_OPENAI_MODEL` environment variable.
pub const MODEL: &str = "gpt-6.1-terra";
/// The quick model that tidies a dictation. `ODM_OPENAI_FAST_MODEL`.
pub const FAST_MODEL: &str = "gpt-6-luna";
/// Speech to text for a recording. `ODM_OPENAI_TRANSCRIBE_MODEL`.
pub const TRANSCRIBE_MODEL: &str = "gpt-transcribe";

const TIMEOUT: Duration = Duration::from_secs(120);

pub fn model() -> String {
    std::env::var("ODM_OPENAI_MODEL").ok().filter(|m| !m.trim().is_empty()).unwrap_or_else(|| MODEL.to_string())
}

pub fn fast_model() -> String {
    std::env::var("ODM_OPENAI_FAST_MODEL").ok().filter(|m| !m.trim().is_empty()).unwrap_or_else(|| FAST_MODEL.to_string())
}

pub fn transcribe_model() -> String {
    std::env::var("ODM_OPENAI_TRANSCRIBE_MODEL").ok().filter(|m| !m.trim().is_empty()).unwrap_or_else(|| TRANSCRIBE_MODEL.to_string())
}

/// The newer model families refuse any temperature but the default, so
/// the field is left out for them.
fn supports_temperature(model: &str) -> bool {
    !(model.starts_with("gpt-5") || model.starts_with("gpt-6") || model.starts_with('o'))
}

/// The form field naming the spoken language: the newer transcription
/// model takes a list.
fn language_field(model: &str) -> &'static str {
    if model.starts_with("gpt-transcribe") {
        "languages[]"
    } else {
        "language"
    }
}

/// A failed request as a sentence, from its status.
fn plain_error(status: u16) -> String {
    match status {
        401 | 403 => "Your OpenAI key was not accepted. Check it in Settings.".into(),
        429 => "OpenAI is busy or your account is out of credit. Try again in a minute.".into(),
        400 | 404 | 422 => "OpenAI could not take this request. If you changed the model, check its name.".into(),
        _ => "OpenAI did not answer just now. Try again in a minute.".into(),
    }
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder().timeout(TIMEOUT).build().map_err(|e| {
        eprintln!("[openai] client: {e}");
        "Could not start a connection. Try again.".to_string()
    })
}

fn chat(key: &str, model: &str, system: &str, prompt: &str, json: bool) -> Result<String, String> {
    let mut body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": prompt},
        ],
    });
    if json {
        body["response_format"] = serde_json::json!({"type": "json_object"});
    }
    if supports_temperature(model) {
        body["temperature"] = serde_json::json!(0.2);
    }
    let key = key.to_string();
    crate::rt::block_on(async move {
        let resp = client()?.post(CHAT_URL).bearer_auth(&key).json(&body).send().await.map_err(|e| {
            eprintln!("[openai] send: {e}");
            "Could not reach OpenAI. Check your internet connection.".to_string()
        })?;
        let status = resp.status();
        let v: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
        if !status.is_success() {
            eprintln!("[openai] {status}: {}", v["error"]["message"].as_str().unwrap_or(""));
            return Err(plain_error(status.as_u16()));
        }
        Ok(v["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string())
    })
}

/// Plain text from the writing model.
pub fn generate_text(key: &str, system: &str, prompt: &str) -> Result<String, String> {
    chat(key, &model(), system, prompt, false)
}

/// One JSON object from the writing model.
pub fn generate_json(key: &str, system: &str, prompt: &str) -> Result<String, String> {
    chat(key, &model(), system, prompt, true)
}

/// One JSON object from the quick model.
pub fn generate_json_fast(key: &str, system: &str, prompt: &str) -> Result<String, String> {
    chat(key, &fast_model(), system, prompt, true)
}

/// A recording (16 kHz mono WAV) to text. `context` is what was said just
/// before, as a hint for names and topic.
pub fn transcribe(key: &str, wav: Vec<u8>, language: Option<&str>, context: Option<&str>) -> Result<String, String> {
    let model = transcribe_model();
    let key = key.to_string();
    let language = language.map(str::to_string);
    let context = context.map(str::to_string);
    crate::rt::block_on(async move {
        let part = reqwest::multipart::Part::bytes(wav).file_name("audio.wav").mime_str("audio/wav").map_err(|e| {
            eprintln!("[openai] audio part: {e}");
            "The recording could not be prepared.".to_string()
        })?;
        let mut form = reqwest::multipart::Form::new().text("model", model.clone()).text("response_format", "json").part("file", part);
        if let Some(lang) = language {
            form = form.text(language_field(&model), lang);
        }
        if let Some(ctx) = context {
            // Bounded: a long hint starts steering the words instead of
            // informing them.
            let tail: String = ctx.chars().rev().take(400).collect::<Vec<_>>().into_iter().rev().collect();
            form = form.text("prompt", tail);
        }
        let resp = client()?.post(TRANSCRIBE_URL).bearer_auth(&key).multipart(form).send().await.map_err(|e| {
            eprintln!("[openai] transcribe send: {e}");
            "Could not reach OpenAI. Check your internet connection.".to_string()
        })?;
        let status = resp.status();
        let v: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
        if !status.is_success() {
            eprintln!("[openai] transcribe {status}: {}", v["error"]["message"].as_str().unwrap_or(""));
            return Err(plain_error(status.as_u16()));
        }
        Ok(v["text"].as_str().unwrap_or("").to_string())
    })
}

/// A short answer written from a web search, with its sources as
/// (title, url).
pub fn web_search(key: &str, instructions: &str, input: &str) -> Result<(String, Vec<(String, String)>), String> {
    let body = serde_json::json!({
        "model": model(),
        "instructions": instructions,
        "input": input,
        "tools": [{"type": "web_search", "search_context_size": "medium"}],
        "reasoning": {"effort": "low"},
    });
    let key = key.to_string();
    crate::rt::block_on(async move {
        let resp = client()?.post(RESPONSES_URL).bearer_auth(&key).json(&body).send().await.map_err(|e| {
            eprintln!("[openai] web search send: {e}");
            "Could not reach OpenAI. Check your internet connection.".to_string()
        })?;
        let status = resp.status();
        let v: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
        if !status.is_success() {
            eprintln!("[openai] web search {status}: {}", v["error"]["message"].as_str().unwrap_or(""));
            return Err(plain_error(status.as_u16()));
        }
        Ok(parse_web_response(&v))
    })
}

/// The text and the cited pages of a Responses reply.
pub fn parse_web_response(v: &serde_json::Value) -> (String, Vec<(String, String)>) {
    let mut text = String::new();
    let mut cites: Vec<(String, String)> = Vec::new();
    for item in v["output"].as_array().cloned().unwrap_or_default() {
        for c in item["content"].as_array().cloned().unwrap_or_default() {
            if let Some(t) = c["text"].as_str() {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(t);
            }
            for a in c["annotations"].as_array().cloned().unwrap_or_default() {
                if let Some(url) = a["url"].as_str() {
                    let title = a["title"].as_str().unwrap_or(url).to_string();
                    if !cites.iter().any(|(_, u)| u == url) {
                        cites.push((title, url.to_string()));
                    }
                }
            }
        }
    }
    (text.trim().to_string(), cites)
}

/// Whether a key answers at all, for the Settings page's check.
pub fn check_key(key: &str) -> Result<(), String> {
    let key = key.to_string();
    crate::rt::block_on(async move {
        let resp = client()?.get("https://api.openai.com/v1/models").bearer_auth(&key).send().await.map_err(|e| {
            eprintln!("[openai] check: {e}");
            "Could not reach OpenAI. Check your internet connection.".to_string()
        })?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(plain_error(resp.status().as_u16()))
        }
    })
}

/// A JSON reply that came wrapped in a code fence, unwrapped.
pub fn strip_fence(raw: &str) -> &str {
    let t = raw.trim();
    let t = t.strip_prefix("```json").or_else(|| t.strip_prefix("```")).unwrap_or(t);
    t.strip_suffix("```").unwrap_or(t).trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_models_get_no_temperature() {
        assert!(!supports_temperature("gpt-6.1-terra"));
        assert!(!supports_temperature("gpt-5-mini"));
        assert!(supports_temperature("gpt-4o"));
    }

    #[test]
    fn the_transcription_language_field_follows_the_model() {
        assert_eq!(language_field("gpt-transcribe"), "languages[]");
        assert_eq!(language_field("gpt-4o-transcribe"), "language");
    }

    #[test]
    fn errors_never_show_a_status_code() {
        for s in [400, 401, 403, 404, 429, 500, 502] {
            let m = plain_error(s);
            assert!(!m.contains(&s.to_string()), "{m}");
        }
    }

    #[test]
    fn a_web_reply_gives_its_text_and_its_sources_once() {
        let v = serde_json::json!({"output": [
            {"type": "web_search_call"},
            {"content": [{"text": "Runs a design studio.", "annotations": [
                {"url": "https://a.example", "title": "A"},
                {"url": "https://a.example", "title": "A again"}
            ]}]}
        ]});
        let (text, cites) = parse_web_response(&v);
        assert_eq!(text, "Runs a design studio.");
        assert_eq!(cites, vec![("A".to_string(), "https://a.example".to_string())]);
    }

    #[test]
    fn a_fenced_reply_is_unwrapped() {
        assert_eq!(strip_fence("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_fence("{\"a\":1}"), "{\"a\":1}");
    }
}
