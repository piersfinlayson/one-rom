// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The host's bus timing and the bursts it produces.

use core::num::NonZeroU8;

use crate::program::MAX_PINS;

/// The shortest segment the drive program runs. Two edges in a burst are
/// together or at least this far apart.
pub const MIN_CYCLES: u32 = 3;

/// The length of the hold that ends every burst, which is the longest segment
/// one word encodes for [`MAX_PINS`] lines.
pub const HOLD_CYCLES: u32 = (1 << (32 - MAX_PINS as u32)) - 1 + MIN_CYCLES;

/// The most segments [`burst`] writes.
pub const MAX_SEGMENTS: usize = 3 * u8::MAX as usize + 3;

/// Cycles the GPIO input synchroniser delays `in pins`.
pub const SYNC_CYCLES: u32 = 2;

/// Lab's system clock, which also clocks PIO.
pub const SYS_CLOCK_HZ: u32 = 150_000_000;

/// A host's bus timing in ns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostProfile {
    /// Each bus cycle's length.
    pub cycle_ns: u32,
    /// Non-ROM cycles before each read.
    pub extra: u8,
    /// Chip select pulse on a non-ROM cycle, or 0 for none.
    pub glitch_ns: u32,
    /// Chip select time at the end of a read.
    pub cs_ns: u32,
    /// Time from a non-ROM cycle's address change to its glitch.
    pub glitch_pos_ns: u32,
    /// 1 in this many non-ROM cycles has a glitch.
    pub glitch_every: NonZeroU8,
}

/// A PAL C64 reading its kernal ROM.
pub const C64: HostProfile = HostProfile {
    cycle_ns: 1015,
    extra: 3,
    glitch_ns: 47,
    cs_ns: 450,
    glitch_pos_ns: 507,
    glitch_every: NonZeroU8::MIN,
};

impl HostProfile {
    /// The profile in PIO cycles at [`SYS_CLOCK_HZ`].
    pub fn to_bus(self) -> Result<Profile, ProfileError> {
        Profile::new(
            cycles(self.cycle_ns),
            self.extra,
            cycles(self.glitch_ns),
            cycles(self.cs_ns),
            cycles(self.glitch_pos_ns),
            self.glitch_every,
        )
    }
}

/// `ns` in PIO cycles at [`SYS_CLOCK_HZ`], to the nearest.
pub fn cycles(ns: u32) -> u32 {
    let hz = u64::from(SYS_CLOCK_HZ);
    let cycles = (u64::from(ns) * hz + 500_000_000) / 1_000_000_000;
    u32::try_from(cycles).unwrap_or(u32::MAX)
}

/// A host's bus timing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    cycle: u32,
    extra: u8,
    glitch: u32,
    cs: u32,
    glitch_pos: u32,
    glitch_every: NonZeroU8,
}

/// A limit a [`Profile`] breaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileError {
    /// `cycle` is over [`HOLD_CYCLES`].
    CycleTooLong,
    /// `glitch` is under [`MIN_CYCLES`].
    GlitchTooShort,
    /// `glitch` is over `cycle` less [`MIN_CYCLES`].
    GlitchTooLong,
    /// The glitch ends after its cycle.
    GlitchOutsideCycle,
    /// `cs` is under [`MIN_CYCLES`].
    CsTooShort,
    /// `cs` is over `cycle`.
    CsTooLong,
    /// Two edges in a cycle are under [`MIN_CYCLES`] apart.
    EdgesTooClose,
    /// The select lines are inactive for under [`MIN_CYCLES`] between a glitch
    /// and the read.
    PulsesTooClose,
}

impl Profile {
    /// A profile with [`HostProfile`]'s fields in PIO cycles.
    pub fn new(
        cycle: u32,
        extra: u8,
        glitch: u32,
        cs: u32,
        glitch_pos: u32,
        glitch_every: NonZeroU8,
    ) -> Result<Profile, ProfileError> {
        let too_close = |gap: u32| gap != 0 && gap < MIN_CYCLES;

        if cycle > HOLD_CYCLES {
            return Err(ProfileError::CycleTooLong);
        }
        if glitch != 0 {
            if glitch < MIN_CYCLES {
                return Err(ProfileError::GlitchTooShort);
            }
            if glitch + MIN_CYCLES > cycle {
                return Err(ProfileError::GlitchTooLong);
            }
            if glitch_pos > cycle - glitch {
                return Err(ProfileError::GlitchOutsideCycle);
            }
        }
        if cs < MIN_CYCLES {
            return Err(ProfileError::CsTooShort);
        }
        if cs > cycle {
            return Err(ProfileError::CsTooLong);
        }

        let after_glitch = (glitch != 0).then(|| cycle - glitch_pos - glitch);
        let before_cs = cycle - cs;
        if after_glitch.is_some_and(|after| too_close(glitch_pos) || too_close(after))
            || too_close(before_cs)
        {
            return Err(ProfileError::EdgesTooClose);
        }
        if extra != 0 && after_glitch.is_some_and(|after| after + before_cs < MIN_CYCLES) {
            return Err(ProfileError::PulsesTooClose);
        }

        Ok(Profile {
            cycle,
            extra,
            glitch,
            cs,
            glitch_pos,
            glitch_every,
        })
    }
}

/// A stretch of a burst with the bus unchanged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Segment {
    pub addr: u32,
    /// The select lines are at their active level.
    pub selected: bool,
    pub cycles: u32,
}

/// The non-ROM address and glitch counters, which continue from one burst to
/// the next.
#[derive(Clone, Copy, Debug)]
pub struct BurstState {
    top_half: u32,
    next: u32,
    until_glitch: u8,
}

impl BurstState {
    /// Non-ROM addresses count up from the start of the top half of a ROM
    /// with `addr_bits` address lines and wrap within it. The first non-ROM
    /// cycle has a glitch.
    ///
    /// # Panics
    ///
    /// If `addr_bits` is 0 or over 32.
    pub fn new(addr_bits: u8) -> BurstState {
        assert!((1..=32).contains(&addr_bits));
        BurstState {
            top_half: 1 << (addr_bits - 1),
            next: 0,
            until_glitch: 0,
        }
    }

    fn next_addr(&mut self) -> u32 {
        let addr = self.top_half | (self.next & (self.top_half - 1));
        self.next = self.next.wrapping_add(1);
        addr
    }

    fn glitch_due(&mut self, every: NonZeroU8) -> bool {
        if self.until_glitch == 0 {
            self.until_glitch = every.get() - 1;
            true
        } else {
            self.until_glitch -= 1;
            false
        }
    }
}

/// What [`burst`] wrote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Burst {
    pub segments: usize,
    /// The cycle whose data R captures, counted from the start of the burst.
    /// It is also R's word.
    pub sample_at: u32,
}

/// Writes the burst for a read of `addr` to `out`. The burst is `extra`
/// non-ROM cycles, the read, then a hold of [`HOLD_CYCLES`] at `addr` with
/// the select lines inactive.
///
/// # Panics
///
/// If `out` is shorter than the burst.
pub fn burst(profile: &Profile, state: &mut BurstState, addr: u32, out: &mut [Segment]) -> Burst {
    let p = profile;
    let mut n = 0;
    let mut put = |addr: u32, selected: bool, cycles: u32| {
        if cycles != 0 {
            out[n] = Segment {
                addr,
                selected,
                cycles,
            };
            n += 1;
        }
    };

    for _ in 0..p.extra {
        let non_rom = state.next_addr();
        if state.glitch_due(p.glitch_every) && p.glitch != 0 {
            put(non_rom, false, p.glitch_pos);
            put(non_rom, true, p.glitch);
            put(non_rom, false, p.cycle - p.glitch_pos - p.glitch);
        } else {
            put(non_rom, false, p.cycle);
        }
    }
    put(addr, false, p.cycle - p.cs);
    put(addr, true, p.cs);
    put(addr, false, HOLD_CYCLES);

    Burst {
        segments: n,
        sample_at: (u32::from(p.extra) + 1) * p.cycle - SYNC_CYCLES,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec;
    use std::vec::Vec;

    /// The bus in one PIO cycle: the address, and which pulse the select
    /// lines are active for.
    type Bus = (u32, Option<usize>);

    fn every(n: u8) -> NonZeroU8 {
        NonZeroU8::new(n).unwrap()
    }

    fn c64() -> Profile {
        C64.to_bus().unwrap()
    }

    #[test]
    fn c64_in_cycles() {
        assert_eq!(c64(), Profile::new(152, 3, 7, 68, 76, every(1)).unwrap());
    }

    fn run(profile: &Profile, state: &mut BurstState, addr: u32) -> (Vec<Segment>, Burst) {
        let mut out = vec![Segment::default(); 3 * usize::from(profile.extra) + 3];
        let b = burst(profile, state, addr, &mut out);
        out.truncate(b.segments);
        (out, b)
    }

    /// The bus in each cycle of a burst, without the hold.
    fn timeline(segments: &[Segment]) -> Vec<Bus> {
        let (hold, timed) = segments.split_last().unwrap();
        assert_eq!(hold.cycles, HOLD_CYCLES);
        assert!(!hold.selected);
        timed
            .iter()
            .enumerate()
            .flat_map(|(i, s)| vec![(s.addr, s.selected.then_some(i)); s.cycles as usize])
            .collect()
    }

    /// Whether every edge on `bus` is together with or at least [`MIN_CYCLES`]
    /// from the next, and the select lines are inactive for at least
    /// [`MIN_CYCLES`] between pulses.
    fn keeps_to_rules(bus: &[Bus]) -> bool {
        let runs = |same: &dyn Fn(&Bus, &Bus) -> bool| {
            let mut runs: Vec<(Bus, usize)> = Vec::new();
            for b in bus {
                match runs.last_mut() {
                    Some((last, len)) if same(last, b) => *len += 1,
                    _ => runs.push((*b, 1)),
                }
            }
            runs
        };
        let min = MIN_CYCLES as usize;
        let edges = runs(&|a, b| a == b);
        let pulses = runs(&|a, b| a.1 == b.1);
        let between = pulses.get(1..pulses.len().saturating_sub(1)).unwrap_or(&[]);
        edges.iter().all(|&(_, len)| len >= min)
            && pulses
                .windows(2)
                .all(|w| w[0].0.1.is_none() || w[1].0.1.is_none())
            && between
                .iter()
                .all(|&((_, p), len)| p.is_some() || len >= min)
    }

    /// The bus a profile describes before any limit applies: two non-ROM
    /// cycles that both glitch, then the read.
    fn described(cycle: u32, glitch: u32, cs: u32, pos: u32) -> Vec<Bus> {
        let mut bus = Vec::new();
        for k in 0..3 {
            for t in 0..cycle {
                let selected = if k < 2 {
                    glitch != 0 && t >= pos && t < pos + glitch
                } else {
                    t + cs >= cycle
                };
                bus.push((k, selected.then_some(k as usize)));
            }
        }
        bus
    }

    #[test]
    fn each_limit() {
        use ProfileError::*;
        let e = every(1);
        let cases = [
            (Profile::new(HOLD_CYCLES + 1, 3, 0, 68, 0, e), CycleTooLong),
            (Profile::new(152, 3, 2, 68, 76, e), GlitchTooShort),
            (Profile::new(152, 3, 150, 68, 0, e), GlitchTooLong),
            (Profile::new(152, 3, 7, 68, 146, e), GlitchOutsideCycle),
            (Profile::new(152, 3, 7, 2, 76, e), CsTooShort),
            (Profile::new(152, 3, 7, 153, 76, e), CsTooLong),
            (Profile::new(152, 3, 7, 68, 1, e), EdgesTooClose),
            (Profile::new(152, 3, 7, 68, 2, e), EdgesTooClose),
            (Profile::new(152, 3, 7, 68, 144, e), EdgesTooClose),
            (Profile::new(152, 3, 7, 150, 76, e), EdgesTooClose),
            (Profile::new(152, 3, 7, 152, 145, e), PulsesTooClose),
        ];
        for (result, err) in cases {
            assert_eq!(result, Err(err));
        }

        // The C64 profile
        assert!(Profile::new(152, 3, 7, 68, 76, e).is_ok());
        assert!(Profile::new(HOLD_CYCLES, 3, 0, 68, 0, e).is_ok());
        assert!(Profile::new(152, 3, 149, 68, 0, e).is_ok());
        assert!(Profile::new(152, 3, 7, 68, 0, e).is_ok());
        assert!(Profile::new(152, 3, 7, 68, 145, e).is_ok());
        assert!(Profile::new(152, 3, 7, 152, 76, e).is_ok());
        assert!(Profile::new(152, 0, 7, 152, 145, e).is_ok());
        assert!(Profile::new(152, 3, 0, 68, 1, e).is_ok());
    }

    /// `Profile::new` accepts a profile exactly when the bus it describes
    /// keeps to the rules.
    #[test]
    fn limits_match_rules() {
        for cycle in 1..=14 {
            for glitch in 0..=cycle + 1 {
                for cs in 0..=cycle + 1 {
                    for pos in 0..=cycle + 1 {
                        let fits = glitch == 0 || pos + glitch <= cycle;
                        let rules = fits
                            && (MIN_CYCLES..=cycle).contains(&cs)
                            && keeps_to_rules(&described(cycle, glitch, cs, pos));
                        let ok = Profile::new(cycle, 2, glitch, cs, pos, every(1)).is_ok();
                        assert_eq!(ok, rules, "cycle {cycle} glitch {glitch} cs {cs} pos {pos}");
                    }
                }
            }
        }
    }

    /// Every burst from an accepted profile keeps to the rules.
    #[test]
    fn bursts_keep_to_rules() {
        for cycle in 3..=12 {
            for glitch in 0..cycle {
                for cs in MIN_CYCLES..=cycle {
                    for pos in 0..cycle {
                        for extra in 0..=3 {
                            for n in 1..=3 {
                                let Ok(p) = Profile::new(cycle, extra, glitch, cs, pos, every(n))
                                else {
                                    continue;
                                };
                                let mut state = BurstState::new(4);
                                for _ in 0..3 {
                                    let (segments, _) = run(&p, &mut state, 1);
                                    assert!(keeps_to_rules(&timeline(&segments)));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn cycles_and_selects() {
        let p = c64();
        let mut state = BurstState::new(13);
        let (segments, b) = run(&p, &mut state, 0x123);
        let bus = timeline(&segments);
        let cycle = p.cycle as usize;
        assert_eq!(bus.len(), cycle * (usize::from(p.extra) + 1));

        for (k, cycle_bus) in bus.chunks(cycle).enumerate() {
            let addr = cycle_bus[0].0;
            assert!(cycle_bus.iter().all(|&(a, _)| a == addr));
            let active: Vec<usize> = (0..cycle).filter(|&t| cycle_bus[t].1.is_some()).collect();
            if k < usize::from(p.extra) {
                assert_eq!(addr, 0x1000 + k as u32);
                let pos = p.glitch_pos as usize;
                assert_eq!(active, (pos..pos + p.glitch as usize).collect::<Vec<_>>());
            } else {
                assert_eq!(addr, 0x123);
                assert_eq!(active, (cycle - p.cs as usize..cycle).collect::<Vec<_>>());
            }
        }

        // R captures the bus 2 cycles before the hold starts.
        assert_eq!(b.sample_at as usize + 2, bus.len());
        assert!(bus[b.sample_at as usize].1.is_some());
    }

    #[test]
    fn segment_count() {
        let mut state = BurstState::new(13);
        let (segments, _) = run(&c64(), &mut state, 0);
        assert_eq!(segments.len(), 3 * 3 + 3);

        let p = Profile::new(HOLD_CYCLES, u8::MAX, 7, 68, 76, every(1)).unwrap();
        let (segments, _) = run(&p, &mut state, 0);
        assert_eq!(segments.len(), MAX_SEGMENTS);
    }

    #[test]
    fn glitch_every_across_bursts() {
        for n in 1..=5 {
            for extra in 0..=4 {
                let p = Profile::new(152, extra, 7, 68, 76, every(n)).unwrap();
                let mut state = BurstState::new(13);
                let mut glitched = Vec::new();
                for _ in 0..10 {
                    let (segments, _) = run(&p, &mut state, 0);
                    let bus = timeline(&segments);
                    for c in bus.chunks(152).take(usize::from(extra)) {
                        glitched.push(c.iter().any(|&(_, s)| s.is_some()));
                    }
                }
                for (i, &g) in glitched.iter().enumerate() {
                    assert_eq!(
                        g,
                        i % usize::from(n) == 0,
                        "every {n} extra {extra} cycle {i}"
                    );
                }
            }
        }
    }

    #[test]
    fn non_rom_addresses_count_up_and_wrap() {
        let p = Profile::new(152, 3, 0, 68, 0, every(1)).unwrap();
        let mut state = BurstState::new(3);
        let mut addrs = Vec::new();
        for _ in 0..3 {
            let (segments, _) = run(&p, &mut state, 0);
            addrs.extend(segments[..3].iter().map(|s| s.addr));
        }
        assert_eq!(addrs, [4, 5, 6, 7, 4, 5, 6, 7, 4]);
    }
}
