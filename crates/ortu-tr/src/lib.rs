//! Turkish detectors and validators for ortu.
//!
//! TCKN, VKN, TR IBAN, payment cards, phone numbers, license plates, names,
//! addresses, birth dates and passport numbers, together with their cue
//! dictionaries and gazetteers (docs/systemdesign.md, section 7.2; algorithms
//! 04 to 09).
//!
//! Detectors are pure functions of their input; session state lives in
//! `ortu-stream`. Implementation lands in milestones M2 and M4.
