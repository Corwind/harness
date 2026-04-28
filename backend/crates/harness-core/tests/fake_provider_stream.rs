//! Behavior test: a hand-rolled fake `LlmProvider` impl emits a
//! hardcoded event sequence and we verify a caller observes exactly
//! those events through the trait surface.
//!
//! This test exercises the trait's contract — that `chat()` returns a
//! `BoxStream<'static, ChatEvent>` and that streaming through it
//! preserves event identity and order — without depending on any
//! adapter crate.

use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use futures::stream::{self, BoxStream, StreamExt};
use harness_core::chat::{ChatEvent, ChatRequest, StopReason, Usage};
use harness_core::error::ProviderError;
use harness_core::ids::ProviderId;
use harness_core::message::{Message, Role};
use harness_core::provider::{
    LlmProvider, ModelInfo, ProviderCapabilities, ProviderConfig,
};
use pretty_assertions::assert_eq;

/// A fake provider that:
///   * advertises a fixed capability matrix and model list
///   * records the most recent `ChatRequest` it was handed
///   * returns the `events` it was constructed with as a stream
struct FakeProvider {
    events: Vec<ChatEvent>,
    last_request: Arc<Mutex<Option<ChatRequest>>>,
}

#[async_trait]
impl LlmProvider for FakeProvider {
    fn id(&self) -> &'static str {
        "fake"
    }

    fn display_name(&self) -> &'static str {
        "Fake"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            tools: true,
            vision: false,
            system_prompt: true,
            max_context_tokens: Some(8_192),
        }
    }

    async fn list_models(&self, _cfg: &ProviderConfig) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![ModelInfo {
            id: "fake-1".into(),
            display_name: "Fake 1".into(),
            context_window: Some(8_192),
            max_output_tokens: Some(2_048),
        }])
    }

    async fn chat(
        &self,
        _cfg: &ProviderConfig,
        request: ChatRequest,
    ) -> Result<BoxStream<'static, ChatEvent>, ProviderError> {
        *self.last_request.lock().unwrap() = Some(request);
        let events = self.events.clone();
        Ok(stream::iter(events).boxed())
    }
}

fn cfg() -> ProviderConfig {
    ProviderConfig {
        provider_id: ProviderId::from_string("fake"),
        config: serde_json::json!({}),
    }
}

#[tokio::test]
async fn fake_provider_streams_exact_event_sequence() {
    let scripted = vec![
        ChatEvent::MessageStart { id: "m1".into() },
        ChatEvent::ContentDelta {
            text: "hello".into(),
        },
        ChatEvent::ContentDelta {
            text: " world".into(),
        },
        ChatEvent::MessageStop {
            stop_reason: StopReason::EndTurn,
            usage: Some(Usage {
                input_tokens: 5,
                output_tokens: 7,
                ..Default::default()
            }),
        },
    ];
    let last_request: Arc<Mutex<Option<ChatRequest>>> = Arc::new(Mutex::new(None));
    let provider = FakeProvider {
        events: scripted.clone(),
        last_request: Arc::clone(&last_request),
    };

    // Consume via the `LlmProvider` trait object to prove the abstraction
    // is what we test, not the concrete type.
    let provider: Arc<dyn LlmProvider> = Arc::new(provider);

    let req = ChatRequest {
        model: "fake-1".into(),
        messages: vec![Message::text(Role::User, "hi")],
        system: None,
        tools: vec![],
        max_tokens: None,
        temperature: None,
    };

    let stream = provider.chat(&cfg(), req.clone()).await.unwrap();
    let observed: Vec<ChatEvent> = stream.collect().await;

    assert_eq!(observed, scripted);
    assert_eq!(last_request.lock().unwrap().as_ref(), Some(&req));
}

#[tokio::test]
async fn fake_provider_streams_tool_use_lifecycle() {
    let scripted = vec![
        ChatEvent::MessageStart { id: "m1".into() },
        ChatEvent::ToolUseStart {
            id: "tu_1".into(),
            name: "echo".into(),
        },
        ChatEvent::ToolUseDelta {
            id: "tu_1".into(),
            partial_json: "{\"text\":".into(),
        },
        ChatEvent::ToolUseDelta {
            id: "tu_1".into(),
            partial_json: "\"hi\"}".into(),
        },
        ChatEvent::ToolUseStop {
            id: "tu_1".into(),
            input: serde_json::json!({"text": "hi"}),
        },
        ChatEvent::MessageStop {
            stop_reason: StopReason::ToolUse,
            usage: None,
        },
    ];
    let provider: Arc<dyn LlmProvider> = Arc::new(FakeProvider {
        events: scripted.clone(),
        last_request: Arc::new(Mutex::new(None)),
    });
    let stream = provider
        .chat(
            &cfg(),
            ChatRequest {
                model: "fake-1".into(),
                messages: vec![],
                system: None,
                tools: vec![],
                max_tokens: None,
                temperature: None,
            },
        )
        .await
        .unwrap();
    let observed: Vec<ChatEvent> = stream.collect().await;
    assert_eq!(observed, scripted);
}

#[tokio::test]
async fn fake_provider_capabilities_and_models_are_observable_via_trait() {
    let provider: Arc<dyn LlmProvider> = Arc::new(FakeProvider {
        events: vec![],
        last_request: Arc::new(Mutex::new(None)),
    });

    assert_eq!(provider.id(), "fake");
    assert_eq!(provider.display_name(), "Fake");
    assert_eq!(
        provider.capabilities(),
        ProviderCapabilities {
            streaming: true,
            tools: true,
            vision: false,
            system_prompt: true,
            max_context_tokens: Some(8_192),
        }
    );

    let models = provider.list_models(&cfg()).await.unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "fake-1");
}
