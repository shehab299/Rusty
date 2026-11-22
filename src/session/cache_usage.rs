//! Cache usage accounting for the current conversation.

use super::{Event, SessionData};

pub fn cache_hit_percent(input_tokens: Option<u64>, cache_hit_tokens: Option<u64>) -> Option<f64> {
    input_tokens
        .filter(|tokens| *tokens > 0)
        .zip(cache_hit_tokens)
        .map(|(input, cached)| cached as f64 / input as f64 * 100.0)
}

/// Token-weighted usage for the current conversation, excluding unknown samples.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CacheUsage {
    pub input_tokens: u64,
    pub cache_hit_tokens: u64,
    pub incomplete: bool,
}

impl CacheUsage {
    pub fn add(&mut self, input_tokens: Option<u64>, cache_hit_tokens: Option<u64>) {
        if let Some((input, cached)) = input_tokens.zip(cache_hit_tokens) {
            self.input_tokens = self.input_tokens.saturating_add(input);
            self.cache_hit_tokens = self.cache_hit_tokens.saturating_add(cached);
        } else {
            self.incomplete = true;
        }
    }
}

impl From<&SessionData> for CacheUsage {
    fn from(data: &SessionData) -> Self {
        let mut usage = Self::default();
        for event in &data.events {
            match event {
                Event::ConversationCleared => usage = Self::default(),
                Event::ModelResponded {
                    input_tokens,
                    cache_hit_tokens,
                    ..
                } => {
                    usage.add(*input_tokens, *cache_hit_tokens);
                }
                _ => {}
            }
        }
        usage
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_sdk::Message;

    fn response(input_tokens: Option<u64>, cache_hit_tokens: Option<u64>) -> Event {
        Event::ModelResponded {
            request_sequence: 0,
            message: Box::new(Message::Assistant {
                content: "Answer".into(),
                tool_calls: Vec::new(),
                provider_details: None,
            }),
            context_tokens: None,
            input_tokens,
            cache_hit_tokens,
        }
    }

    #[test]
    fn cache_hit_percent_requires_known_usage_and_positive_input() {
        assert_eq!(cache_hit_percent(Some(100), Some(25)), Some(25.0));
        assert_eq!(cache_hit_percent(Some(100), Some(0)), Some(0.0));
        assert_eq!(cache_hit_percent(Some(0), Some(0)), None);
        assert_eq!(cache_hit_percent(None, Some(25)), None);
        assert_eq!(cache_hit_percent(Some(100), None), None);
    }

    #[test]
    fn resumed_usage_matches_live_usage_after_clear() {
        let data = SessionData {
            events: vec![
                response(Some(100), Some(90)),
                response(None, None),
                Event::ConversationCleared,
                response(Some(300), Some(60)),
                response(Some(200), None),
            ],
            ..Default::default()
        };
        let mut live = CacheUsage::default();
        live.add(Some(300), Some(60));
        live.add(Some(200), None);
        assert_eq!(CacheUsage::from(&data), live);
        assert_eq!(live.input_tokens, 300);
        assert_eq!(live.cache_hit_tokens, 60);
        assert!(live.incomplete);
    }

    #[test]
    fn clear_resets_usage_and_missing_telemetry() {
        let data = SessionData {
            events: vec![response(Some(100), None), Event::ConversationCleared],
            ..Default::default()
        };
        assert_eq!(CacheUsage::from(&data), CacheUsage::default());
    }
}
