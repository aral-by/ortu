//! M3 acceptance (docs/systemdesign.md, section 17): an agent question and the
//! caller's answer, values split over two and three turns in both modes,
//! no hold-back in Realtime, and the four boundaries of the LLM loop.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers fail the test on setup errors; allow-unwrap-in-tests covers #[test] functions only"
)]

use std::sync::{Arc, Mutex};

use ortu_core::{
    AuditAction, AuditError, AuditEvent, AuditSink, EntityKind, Policy, SecretValue, Speaker,
    Transcript, Turn, ValidatorOutcome,
};
use ortu_crypto::{SecretKey, StaticKeyProvider};
use ortu_stream::{Engine, EngineBuilder, Error, GuardError, ReadBack, Session};
use serde_json::json;

fn builder() -> EngineBuilder {
    Engine::builder().key_provider(StaticKeyProvider::single(SecretKey::from_bytes([42; 32])))
}

fn session() -> Session {
    builder()
        .build()
        .unwrap()
        .session("call-7", "tenant")
        .unwrap()
}

/// Pushes the turns, alternating as written, and returns the masked texts.
fn push_all(session: &mut Session, turns: &[(Speaker, &str)]) -> Vec<String> {
    turns
        .iter()
        .zip(0u32..)
        .map(|((speaker, text), index)| {
            let turn = Turn::from_text(
                index,
                *speaker,
                text,
                u64::from(index) * 1000,
                u64::from(index) * 1000 + 900,
            );
            session.push(turn).unwrap().text
        })
        .collect()
}

fn is_token(text: &str, label: &str) -> bool {
    text.starts_with(&format!("[{label}:")) && text.ends_with(']') && !text.contains(' ')
}

const BOT: Speaker = Speaker::Bot;
const CALLER: Speaker = Speaker::Caller;

#[test]
fn a_tckn_split_over_two_turns_with_a_backchannel_between() {
    let mut session = session();
    let texts = push_all(
        &mut session,
        &[
            (BOT, "TC kimlik numaranızı alabilir miyim?"),
            (CALLER, "on sıfır sıfır sıfır sıfır"),
            (BOT, "evet"),
            (CALLER, "sıfır sıfır yüz kırk altı"),
        ],
    );
    assert_eq!(texts[0], "TC kimlik numaranızı alabilir miyim?");
    assert!(is_token(&texts[1], "TCKN_INCOMPLETE"), "{}", texts[1]);
    assert_eq!(texts[2], "evet");
    assert!(is_token(&texts[3], "TCKN"), "{}", texts[3]);

    // The completing turn's token resolves to the whole value.
    let released = session
        .guard_tool_args("query_debt", &json!({ "tckn": texts[3] }))
        .unwrap();
    assert_eq!(released["tckn"], "10000000146");
}

#[test]
fn a_tckn_split_over_three_turns_in_both_modes() {
    let mut session = session();
    let turns = [
        (CALLER, "TC'm on sıfır sıfır"),
        (BOT, "hı hı"),
        (CALLER, "sıfır sıfır sıfır sıfır"),
        (BOT, "devam edin"),
        (CALLER, "yüz kırk altı"),
    ];
    let texts = push_all(&mut session, &turns);
    assert!(
        texts[0].starts_with("TC'm [TCKN_INCOMPLETE:"),
        "{}",
        texts[0]
    );
    assert!(is_token(&texts[2], "TCKN_INCOMPLETE"), "{}", texts[2]);
    assert!(is_token(&texts[4], "TCKN"), "{}", texts[4]);

    let post = session.finish();
    let tckns: Vec<_> = post
        .entities
        .iter()
        .filter(|e| e.kind == EntityKind::Tckn)
        .collect();
    assert_eq!(tckns.len(), 1, "{:#?}", post.entities);
    let entity = tckns[0];
    assert!(entity.cross_turn);
    assert_eq!(entity.validator, ValidatorOutcome::Passed);
    let turns: Vec<u32> = entity.parts.iter().map(|p| p.turn).collect();
    assert_eq!(turns, [0, 2, 4]);
    // The persistent transcript carries the full-value token in every part,
    // and it is the token the LLM saw in Realtime.
    assert_eq!(entity.replacement, texts[4]);
    assert_eq!(post.turns[0].text, format!("TC'm {}", texts[4]));
    assert_eq!(post.turns[2].text, texts[4]);
    assert_eq!(post.turns[1].text, "hı hı");
}

#[test]
fn no_raw_digit_of_a_split_value_reaches_realtime_output() {
    let mut session = session();
    let texts = push_all(
        &mut session,
        &[
            (BOT, "TC kimliğinizi söyler misiniz"),
            (CALLER, "bir sıfır sıfır sıfır"),
            (BOT, "evet"),
            (CALLER, "sıfır sıfır sıfır sıfır"),
            (CALLER, "bir dört altı"),
        ],
    );
    for text in &texts[1..] {
        for word in ["bir", "sıfır", "dört", "altı"] {
            assert!(!text.split(' ').any(|w| w == word), "{text}");
        }
    }
    assert!(is_token(&texts[4], "TCKN"), "{}", texts[4]);
}

#[test]
fn a_phone_number_split_after_a_cue() {
    let mut session = session();
    let texts = push_all(
        &mut session,
        &[
            (CALLER, "telefon numaram sıfır beş yüz otuz iki"),
            (CALLER, "yüz yirmi üç kırk beş altmış yedi"),
        ],
    );
    assert!(
        texts[0].starts_with("telefon numaram [PHONE_INCOMPLETE:"),
        "{}",
        texts[0]
    );
    assert!(is_token(&texts[1], "PHONE"), "{}", texts[1]);
    let released = session
        .guard_tool_args("sms", &json!({ "to": texts[1] }))
        .unwrap();
    assert_eq!(released["to"], "+905321234567");
}

#[test]
fn asking_again_for_the_same_kind_keeps_the_run_open() {
    let mut session = session();
    let texts = push_all(
        &mut session,
        &[
            (BOT, "TC kimlik numaranızı alabilir miyim?"),
            (CALLER, "on sıfır sıfır sıfır sıfır"),
            (BOT, "Kimlik numaranızı alabilir miyim?"),
            (CALLER, "sıfır sıfır yüz kırk altı"),
        ],
    );
    assert!(is_token(&texts[1], "TCKN_INCOMPLETE"), "{}", texts[1]);
    assert!(is_token(&texts[3], "TCKN"), "{}", texts[3]);

    let post = session.finish();
    assert_eq!(post.turns[1].text, texts[3], "{:#?}", post.turns);
    assert_eq!(post.turns[3].text, texts[3]);
    assert_eq!(post.entities.len(), 1, "{:#?}", post.entities);
}

#[test]
fn a_value_that_is_never_finished_stays_masked_in_both_modes() {
    let mut session = session();
    let texts = push_all(
        &mut session,
        &[
            (BOT, "TC kimlik numaranız?"),
            (CALLER, "on sıfır sıfır sıfır sıfır"),
            (BOT, "Adresiniz nedir?"),
            (CALLER, "Çankaya Ankara"),
        ],
    );
    assert!(is_token(&texts[1], "TCKN_INCOMPLETE"), "{}", texts[1]);
    let post = session.finish();
    assert!(
        is_token(&post.turns[1].text, "TCKN_INCOMPLETE"),
        "{}",
        post.turns[1].text
    );
    assert_eq!(
        post.entities[0].validator,
        ValidatorOutcome::Failed(ortu_core::FailureReason::TooShort)
    );
}

#[test]
fn an_unfinished_value_inside_a_turn_is_masked_without_opening_a_run() {
    let mut session = session();
    let texts = push_all(
        &mut session,
        &[
            (CALLER, "TC'm on iki üç, tamam mı"),
            (CALLER, "dört beş altı"),
        ],
    );
    assert!(
        texts[0].starts_with("TC'm [TCKN_INCOMPLETE:"),
        "{}",
        texts[0]
    );
    assert!(texts[0].ends_with(" tamam mı"), "{}", texts[0]);
    assert_eq!(texts[1], "dört beş altı");
}

#[test]
fn a_substantive_assistant_turn_or_a_new_topic_closes_the_run() {
    let mut session = session();
    let texts = push_all(
        &mut session,
        &[
            (CALLER, "TC'm on bir iki"),
            (BOT, "Hangi ilçede oturuyorsunuz?"),
            (CALLER, "üç dört beş"),
            (CALLER, "TC'm on bir iki"),
            (CALLER, "Çankaya'da oturuyorum"),
            (CALLER, "üç dört beş"),
        ],
    );
    assert!(texts[0].contains("TCKN_INCOMPLETE"));
    assert_eq!(texts[2], "üç dört beş");
    assert_eq!(texts[5], "üç dört beş");
}

#[test]
fn a_run_stays_open_for_at_most_max_open_turns() {
    let mut session = builder()
        .max_open_turns(2)
        .build()
        .unwrap()
        .session("c", "t")
        .unwrap();
    let texts = push_all(
        &mut session,
        &[
            (CALLER, "TC'm bir iki"),
            (CALLER, "üç dört"),
            (CALLER, "beş altı"),
        ],
    );
    assert!(texts[1].contains("TCKN_INCOMPLETE"), "{}", texts[1]);
    assert_eq!(texts[2], "beş altı");
}

#[test]
fn turns_must_arrive_in_order() {
    let mut session = session();
    session
        .push(Turn::from_text(3, CALLER, "merhaba", 0, 1))
        .unwrap();
    let error = session
        .push(Turn::from_text(3, CALLER, "tekrar", 1, 2))
        .unwrap_err();
    assert_eq!(
        error,
        Error::TurnOutOfOrder {
            previous: 3,
            pushed: 3
        }
    );
}

#[test]
fn post_call_equals_batch_redact() {
    let engine = builder().build().unwrap();
    let turns: Vec<Turn> = [
        (BOT, "Adınız soyadınız nedir?"),
        (CALLER, "Ahmet Yılmaz"),
        (BOT, "TC kimlik numaranız?"),
        (CALLER, "on sıfır sıfır sıfır sıfır"),
        (BOT, "evet"),
        (
            CALLER,
            "sıfır sıfır yüz kırk altı, telefonum 0532 123 45 67",
        ),
    ]
    .iter()
    .zip(0u32..)
    .map(|((speaker, text), i)| Turn::from_text(i, *speaker, text, 0, 1))
    .collect();
    let mut session = engine.session("call-9", "tenant").unwrap();
    for turn in &turns {
        session.push(turn.clone()).unwrap();
    }
    let finished = session.finish();
    let batch = engine
        .redact(&Transcript {
            call_id: "call-9".to_owned(),
            tenant_id: "tenant".to_owned(),
            channel: ortu_core::Channel::Speech,
            turns,
        })
        .unwrap();
    assert_eq!(finished, batch);
    let kinds: Vec<EntityKind> = batch.entities.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        [EntityKind::FullName, EntityKind::Tckn, EntityKind::Phone]
    );
}

#[test]
fn registered_and_spoken_values_share_a_token() {
    let mut session = session();
    let token = session.register(EntityKind::Phone, &SecretValue::new("0 532 123 45 67"));
    let masked = session
        .push(Turn::from_text(
            0,
            CALLER,
            "numaram sıfır beş yüz otuz iki yüz yirmi üç kırk beş altmış yedi",
            0,
            1,
        ))
        .unwrap();
    assert_eq!(masked.text, format!("numaram {token}"));
}

#[test]
fn tool_results_are_masked_by_field_names_and_content() {
    let mut session = session();
    let result = session.mask_tool_result(
        "query_debt",
        &json!({
            "name": "Ahmet Yılmaz",
            "tckn": 10_000_000_146_u64,
            "contact": { "phoneNumber": "0532 123 45 67" },
            "iban": "TR33 0006 1005 1978 6457 8413 26",
            "note": "Borç 1250 TL, son ödeme 15 Kasım",
            "debt": 1250,
            "active": true
        }),
    );
    assert!(
        result["name"].as_str().unwrap().starts_with("[NAME:"),
        "{result}"
    );
    assert!(
        result["tckn"].as_str().unwrap().starts_with("[TCKN:"),
        "{result}"
    );
    assert!(
        result["contact"]["phoneNumber"]
            .as_str()
            .unwrap()
            .starts_with("[PHONE:")
    );
    assert!(
        result["iban"].as_str().unwrap().starts_with("[IBAN:"),
        "{result}"
    );
    assert_eq!(result["note"], "Borç 1250 TL, son ödeme 15 Kasım");
    assert_eq!(result["debt"], 1250);
    assert_eq!(result["active"], true);

    // The LLM can now pass the tokens on, and reads the name back in full.
    let args = json!({ "customer": result["name"], "tckn": result["tckn"] });
    let released = session.guard_tool_args("send_letter", &args).unwrap();
    assert_eq!(released["customer"], "Ahmet Yılmaz");
    assert_eq!(released["tckn"], "10000000146");
    let reply = format!(
        "Sayın {}, kaydınız bulundu.",
        result["name"].as_str().unwrap()
    );
    assert_eq!(
        session.rehydrate(&reply).unwrap().text.expose_secret(),
        "Sayın Ahmet Yılmaz, kaydınız bulundu."
    );
}

#[test]
fn raw_identifiers_in_tool_arguments_are_rejected() {
    let session = session();
    for (args, kind) in [
        (json!({ "tckn": "10000000146" }), EntityKind::Tckn),
        (json!({ "tckn": 10_000_000_146_u64 }), EntityKind::Tckn),
        (json!({ "q": "kişi 10000000146" }), EntityKind::Tckn),
        (json!({ "to": "+90 532 123 45 67" }), EntityKind::Phone),
        (
            json!({ "iban": "TR330006100519786457841326" }),
            EntityKind::Iban,
        ),
        (json!({ "tckn": "1000000014" }), EntityKind::Tckn),
    ] {
        assert_eq!(
            session.guard_tool_args("t", &args),
            Err(GuardError::UnknownValue { kind }),
            "{args}"
        );
    }
}

#[test]
fn ordinary_arguments_and_public_numbers_pass() {
    let session = session();
    let args = json!({
        "city": "Ankara",
        "amount": 1250,
        "date": "2026-11-15",
        "callback": "444 1 234",
        "ids": [1, 2, 3],
        "flag": null
    });
    assert_eq!(session.guard_tool_args("t", &args).unwrap(), args);
}

#[test]
fn unknown_and_corrupted_tokens_never_pass() {
    let mut session = session();
    let token = session.register(EntityKind::Tckn, &SecretValue::new("10000000146"));
    let digest = token.digest();
    let altered = format!(
        "[TCKN:{}{}]",
        &digest[..7],
        if digest.ends_with('0') { '1' } else { '0' }
    );
    assert_eq!(
        session.guard_tool_args("t", &json!({ "tckn": altered })),
        Err(GuardError::UnknownToken {
            label: "TCKN".to_owned()
        })
    );
    for form in [
        format!("tckn:{digest}"),
        format!("[ TCKN : {} ]", digest.to_ascii_uppercase()),
        format!("TCKN {digest}"),
    ] {
        let released = session
            .guard_tool_args("t", &json!({ "tckn": form }))
            .unwrap();
        assert_eq!(released["tckn"], "10000000146", "{form}");
    }
}

#[test]
fn incomplete_values_reach_only_tools_that_accept_them() {
    let engine = builder()
        .accept_invalid("verify_tckn", EntityKind::Tckn)
        .build()
        .unwrap();
    let mut session = engine.session("c", "t").unwrap();
    let token = session.register(EntityKind::Tckn, &SecretValue::new("100000001"));
    assert!(token.as_str().starts_with("[TCKN_INCOMPLETE:"));
    let args = json!({ "tckn": token.as_str() });
    assert_eq!(
        session.guard_tool_args("query_debt", &args),
        Err(GuardError::InvalidValue {
            kind: EntityKind::Tckn
        })
    );
    assert_eq!(
        session.guard_tool_args("verify_tckn", &args).unwrap()["tckn"],
        "100000001"
    );
}

#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<AuditEvent>>>);

impl AuditSink for Recorder {
    fn record(&self, event: &AuditEvent) -> Result<(), AuditError> {
        self.0.lock().unwrap().push(event.clone());
        Ok(())
    }
}

struct Broken;

impl AuditSink for Broken {
    fn record(&self, _event: &AuditEvent) -> Result<(), AuditError> {
        Err(AuditError::new("store unavailable"))
    }
}

#[test]
fn every_guard_decision_is_audited_without_values() {
    let recorder = Recorder::default();
    let engine = builder().audit_sink(recorder.clone()).build().unwrap();
    let mut session = engine.session("call-1", "tenant-1").unwrap();
    let token = session.register(EntityKind::Tckn, &SecretValue::new("10000000146"));
    session
        .guard_tool_args("query_debt", &json!({ "tckn": token.as_str() }))
        .unwrap();
    session
        .guard_tool_args("query_debt", &json!({ "tckn": "10000000164" }))
        .unwrap_err();

    let events = recorder.0.lock().unwrap().clone();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].action, AuditAction::ToolArgsReleased);
    assert_eq!(events[0].kinds, [EntityKind::Tckn]);
    assert_eq!(events[1].action, AuditAction::ToolCallRejected);
    assert_eq!(events[1].reason, Some("unknown_value"));
    for event in &events {
        let printed = format!("{event:?}");
        assert!(!printed.contains("10000000146") && !printed.contains("10000000164"));
        assert_eq!(event.call_id, "call-1");
        assert_eq!(event.tool.as_deref(), Some("query_debt"));
    }
}

#[test]
fn nothing_is_released_when_the_audit_sink_fails() {
    let engine = builder().audit_sink(Broken).build().unwrap();
    let mut session = engine.session("c", "t").unwrap();
    let token = session.register(EntityKind::Tckn, &SecretValue::new("10000000146"));
    let result = session.guard_tool_args("t", &json!({ "tckn": token.as_str() }));
    assert!(matches!(result, Err(GuardError::Audit(_))));
}

#[test]
fn read_back_follows_the_policy() {
    let engine = builder()
        .read_back(EntityKind::Phone, ReadBack::Full)
        .build()
        .unwrap();
    let mut session = engine.session("c", "t").unwrap();
    let tckn = session.register(EntityKind::Tckn, &SecretValue::new("10000000146"));
    let phone = session.register(EntityKind::Phone, &SecretValue::new("05321234567"));
    let name = session.register(EntityKind::FullName, &SecretValue::new("Ayşe Kaya"));
    let reply = format!("{name}, TC {tckn}, telefon {phone}.");
    let spoken = session.rehydrate(&reply).unwrap();
    assert_eq!(
        spoken.text.expose_secret(),
        "Ayşe Kaya, TC *******0146, telefon +905321234567."
    );
    assert_eq!(spoken.replaced, 3);
    assert!(matches!(
        session.rehydrate("[IBAN:00000000]"),
        Err(GuardError::UnknownToken { .. })
    ));
}

#[test]
fn streaming_rehydration_holds_only_a_possible_token() {
    let mut session = session();
    let name = session.register(EntityKind::FullName, &SecretValue::new("Ayşe Kaya"));
    let (head, tail) = name.as_str().split_at(5);
    let mut rehydrator = session.rehydrator();
    let first = rehydrator.feed(&format!("Merhaba {head}")).unwrap();
    assert_eq!(
        first.expose_secret(),
        "Merhaba ",
        "plain text is not delayed"
    );
    let second = rehydrator.feed(&format!("{tail}, nasılsınız?")).unwrap();
    assert_eq!(second.expose_secret(), "Ayşe Kaya, nasılsınız?");
    assert_eq!(rehydrator.finish().unwrap().expose_secret(), "");
}

#[test]
fn the_engine_refuses_to_start_without_keys_or_with_encryption() {
    assert!(matches!(Engine::builder().build(), Err(Error::Config(_))));
    let encrypt =
        Policy::from_toml_str("[policy]\n[policy.kinds.tckn]\ntransform = \"encrypt\"\n").unwrap();
    assert!(matches!(
        builder().policy(encrypt).build(),
        Err(Error::Config(_))
    ));
}

#[test]
fn redact_and_mask_transforms_do_not_enter_the_map() {
    let policy = Policy::from_toml_str(
        "[policy]\n[policy.kinds.tckn]\ntransform = \"mask\"\n[policy.kinds.phone]\ntransform = \"redact\"\n",
    )
    .unwrap();
    let mut session = builder()
        .policy(policy)
        .build()
        .unwrap()
        .session("c", "t")
        .unwrap();
    let masked = session
        .push(Turn::from_text(
            0,
            CALLER,
            "TC'm 10000000146 telefonum 05321234567",
            0,
            1,
        ))
        .unwrap();
    assert_eq!(masked.text, "TC'm *******0146 telefonum [PHONE]");
    assert_eq!(
        masked.entities[0].validator,
        ValidatorOutcome::Passed,
        "{:#?}",
        masked.entities
    );
}
