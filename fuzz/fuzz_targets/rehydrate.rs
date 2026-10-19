//! Arbitrary LLM output through rehydration: no panic, and streaming at the
//! input's own chunk boundaries equals rehydrating the whole text. The first
//! byte chooses the chunk size; the rest is the text.

#![no_main]

use libfuzzer_sys::fuzz_target;
use ortu_core::{EntityKind, SecretValue};
use ortu_crypto::{SecretKey, StaticKeyProvider};
use ortu_stream::Engine;

fuzz_target!(|data: &[u8]| {
    let Some((&size, rest)) = data.split_first() else {
        return;
    };
    let text = String::from_utf8_lossy(rest);
    let Ok(engine) = Engine::builder()
        .key_provider(StaticKeyProvider::single(SecretKey::from_bytes([5; 32])))
        .build()
    else {
        return;
    };
    let Ok(mut session) = engine.session("call", "tenant") else {
        return;
    };
    session.register(EntityKind::Tckn, &SecretValue::new("10000000146"));
    session.register(EntityKind::FullName, &SecretValue::new("Ahmet Yılmaz"));

    let whole = session
        .rehydrate(&text)
        .map(|r| r.text.expose_secret().to_owned())
        .ok();
    let mut rehydrator = session.rehydrator();
    let mut streamed = Some(String::new());
    let mut chunk = String::new();
    for c in text.chars() {
        chunk.push(c);
        if chunk.len() >= usize::from(size % 16) + 1 {
            match rehydrator.feed(&chunk) {
                Ok(out) => streamed.as_mut().map(|s| s.push_str(out.expose_secret())),
                Err(_) => streamed.take().map(|_| ()),
            };
            chunk.clear();
            if streamed.is_none() {
                break;
            }
        }
    }
    if let Some(s) = streamed.as_mut() {
        match rehydrator.feed(&chunk).and_then(|out| {
            s.push_str(out.expose_secret());
            rehydrator.finish()
        }) {
            Ok(out) => s.push_str(out.expose_secret()),
            Err(_) => streamed = None,
        }
    }
    assert_eq!(streamed, whole);
});
