//! Goal tests for injected `Clock` and `IdGen` (#3, spec §14.2).

#[cfg(feature = "goal")]
use hali::core::{Clock, FixedClock, IdGen, SeededIdGen};

#[cfg(feature = "goal")]
fn is_uuid_v7(s: &str) -> bool {
    let b = s.as_bytes();
    s.len() == 36
        && [8, 13, 18, 23].iter().all(|&i| b[i] == b'-')
        && s.chars()
            .enumerate()
            .all(|(i, c)| [8, 13, 18, 23].contains(&i) || matches!(c, '0'..='9' | 'a'..='f'))
        && b[14] == b'7'
        && matches!(b[19], b'8' | b'9' | b'a' | b'b')
}

#[cfg(feature = "goal")]
#[test]
fn seeded_idgen_is_deterministic_uuidv7() {
    let a = SeededIdGen::new(42);
    let b = SeededIdGen::new(42);
    let first: Vec<String> = (0..3).map(|_| a.next_id()).collect();
    let second: Vec<String> = (0..3).map(|_| b.next_id()).collect();
    assert_eq!(first, second, "same seed, same sequence");
    assert!(first.iter().all(|id| is_uuid_v7(id)), "{first:?}");
    assert_ne!(first[0], first[1]);
    assert_ne!(first[1], first[2]);

    let other = SeededIdGen::new(43).next_id();
    assert_ne!(other, first[0], "different seed, different ids");
}

#[cfg(feature = "goal")]
#[test]
fn fixed_clock_returns_the_injected_instant() {
    let clock = FixedClock::new("2026-10-24T09:30:00Z");
    assert_eq!(clock.now(), "2026-10-24T09:30:00Z");
    assert_eq!(clock.now(), "2026-10-24T09:30:00Z");
}
