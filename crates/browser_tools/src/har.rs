use crate::network::{NetworkRequest, NetworkStore};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::collections::HashMap;

/// Export the store as a HAR `log` object (interchange format).
pub fn export_har(store: &NetworkStore) -> Value {
    let entries: Vec<Value> = store.requests().map(har_entry).collect();
    json!({
        "log": {
            "version": "1.2",
            "creator": { "name": "Zed", "version": "0.1" },
            "entries": entries,
        }
    })
}

/// Import a HAR `log` object into a typed store.
pub fn import_har(value: &Value) -> Result<NetworkStore> {
    let entries = value
        .get("log")
        .and_then(|log| log.get("entries"))
        .and_then(Value::as_array)
        .context("HAR log has no entries array")?;

    let mut store = NetworkStore::new();
    for (index, entry) in entries.iter().enumerate() {
        store.insert(NetworkRequest {
            request_id: format!("har-{index}"),
            url: entry
                .get("request")
                .and_then(|request| string_field(request, "url"))
                .unwrap_or_default(),
            method: entry
                .get("request")
                .and_then(|request| string_field(request, "method"))
                .unwrap_or_else(|| "GET".to_string()),
            status: entry
                .get("response")
                .and_then(|response| u32_field(response, "status")),
            status_text: entry
                .get("response")
                .and_then(|response| string_field(response, "statusText")),
            mime_type: entry
                .get("response")
                .and_then(|response| response.get("content"))
                .and_then(|content| string_field(content, "mimeType")),
            request_headers: har_headers(entry.get("request")),
            response_headers: har_headers(entry.get("response")),
            post_data: entry
                .get("request")
                .and_then(|request| request.get("postData"))
                .and_then(|post_data| string_field(post_data, "text")),
            encoded_data_length: entry
                .get("response")
                .and_then(|response| response.get("content"))
                .and_then(|content| content.get("size"))
                .and_then(Value::as_f64),
            ..Default::default()
        });
    }
    Ok(store)
}

fn har_entry(request: &NetworkRequest) -> Value {
    json!({
        "request": {
            "method": request.method,
            "url": request.url,
            "headers": har_headers_from_map(&request.request_headers),
            "postData": request.post_data.as_ref().map(|text| json!({
                "mimeType": request.request_headers.get("content-type").cloned().unwrap_or_default(),
                "text": text,
            })),
        },
        "response": {
            "status": request.status.unwrap_or(0),
            "statusText": request.status_text.clone().unwrap_or_default(),
            "headers": har_headers_from_map(&request.response_headers),
            "content": {
                "size": request.encoded_data_length.unwrap_or(0.0),
                "mimeType": request.mime_type.clone().unwrap_or_default(),
            },
        },
    })
}

fn har_headers_from_map(headers: &HashMap<String, String>) -> Vec<Value> {
    headers
        .iter()
        .map(|(name, value)| json!({ "name": name, "value": value }))
        .collect()
}

fn har_headers(value: Option<&Value>) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    if let Some(array) = value
        .and_then(|value| value.get("headers"))
        .and_then(Value::as_array)
    {
        for header in array {
            if let (Some(name), Some(value)) =
                (string_field(header, "name"), string_field(header, "value"))
            {
                headers.insert(name, value);
            }
        }
    }
    headers
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn u32_field(value: &Value, key: &str) -> Option<u32> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .map(|value| value as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn har_round_trip_preserves_request_metadata() {
        let mut store = NetworkStore::new();
        store.ingest(&json!({
            "method": "Network.requestWillBeSent",
            "params": {
                "requestId": "1",
                "type": "XHR",
                "request": {
                    "url": "https://example.com/api/items",
                    "method": "POST",
                    "headers": { "Content-Type": "application/json" },
                    "postData": "{\"q\":\"x\"}"
                }
            }
        }));
        store.ingest(&json!({
            "method": "Network.responseReceived",
            "params": {
                "requestId": "1",
                "response": {
                    "status": 200,
                    "statusText": "OK",
                    "mimeType": "application/json"
                }
            }
        }));

        let har = export_har(&store);
        let imported = import_har(&har).expect("HAR should round-trip");

        assert_eq!(imported.len(), 1);
        let request = imported.requests().next().expect("one request");
        assert_eq!(request.url, "https://example.com/api/items");
        assert_eq!(request.method, "POST");
        assert_eq!(request.status, Some(200));
        assert_eq!(request.post_data.as_deref(), Some("{\"q\":\"x\"}"));
        assert_eq!(
            request
                .request_headers
                .get("Content-Type")
                .map(String::as_str),
            Some("application/json")
        );
    }
}
