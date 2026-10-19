//! Arbitrary transcripts through the whole pipeline in PostCall mode: no
//! panic, every entity part inside its turn, one output turn per input turn.
//! Lines are turns; a line starting with `>` is spoken by the bot.

#![no_main]

use libfuzzer_sys::fuzz_target;
use ortu_core::{Channel, Speaker, Transcript, Turn};
use ortu_crypto::{SecretKey, StaticKeyProvider};
use ortu_stream::Engine;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let turns: Vec<Turn> = text
        .lines()
        .zip(0u32..)
        .map(|(line, index)| match line.strip_prefix('>') {
            Some(bot) => Turn::from_text(index, Speaker::Bot, bot, 0, 1000),
            None => Turn::from_text(index, Speaker::Caller, line, 0, 1000),
        })
        .collect();
    let Ok(engine) = Engine::builder()
        .key_provider(StaticKeyProvider::single(SecretKey::from_bytes([5; 32])))
        .build()
    else {
        return;
    };
    let transcript = Transcript {
        call_id: "call".to_owned(),
        tenant_id: "tenant".to_owned(),
        channel: Channel::Speech,
        turns: turns.clone(),
    };
    let Ok(result) = engine.redact(&transcript) else {
        return;
    };
    assert_eq!(result.turns.len(), turns.len());
    for entity in &result.entities {
        for part in &entity.parts {
            let turn = &turns[part.turn as usize];
            assert!(part.words.start < part.words.end);
            assert!(part.words.end as usize <= turn.words.len());
        }
    }
});
