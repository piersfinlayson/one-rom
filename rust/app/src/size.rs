// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! A One ROM's board size, from runtime info or OTP.

use core::future::Future;

use onerom_config::hw::BoardSize;
use onerom_metadata::{MaybeKnown, OneromBoardSize};

/// The board size a device records.
///
/// Runtime info records the size while One ROM runs, from firmware 0.8.0.
/// Earlier firmware records `BoardSizeUnknown`, and a stopped One ROM doesn't
/// have runtime info, so the size then comes from OTP.
///
/// It's `runtime` where that's a size other than `BoardSizeUnknown`.
/// Otherwise it calls `read_otp` and returns the size that reads, or `runtime`
/// where it returns `None`.
pub async fn device_board_size<F, Fut>(
    runtime: Option<MaybeKnown<OneromBoardSize>>,
    read_otp: F,
) -> Option<MaybeKnown<OneromBoardSize>>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Option<OneromBoardSize>>,
{
    match runtime {
        Some(size) if size != MaybeKnown::Known(OneromBoardSize::BoardSizeUnknown) => Some(size),
        Some(_) | None => read_otp().await.map(MaybeKnown::Known).or(runtime),
    }
}

/// The size of a board whose recorded size is `size`. `None` where `size` is
/// neither M nor L or isn't known.
pub fn known_board_size(size: Option<MaybeKnown<OneromBoardSize>>) -> Option<BoardSize> {
    match size? {
        MaybeKnown::Known(size) => known_size(size),
        MaybeKnown::Unknown(_) => None,
    }
}

/// `size` as a [`BoardSize`]. `None` where it's neither M nor L.
pub(crate) fn known_size(size: OneromBoardSize) -> Option<BoardSize> {
    match size {
        OneromBoardSize::BoardSizeM => Some(BoardSize::M),
        OneromBoardSize::BoardSizeL => Some(BoardSize::L),
        OneromBoardSize::BoardSizeUnknown | OneromBoardSize::BoardSizeOther => None,
    }
}
