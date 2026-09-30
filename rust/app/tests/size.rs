// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for where a device's board size comes from and the size a tool uses.

use core::cell::Cell;

use onerom_app::{BoardSize, board_size_or_m, device_board_size};
use onerom_metadata::{MaybeKnown, OneromBoardSize};

use OneromBoardSize::{BoardSizeL, BoardSizeM, BoardSizeOther, BoardSizeUnknown};

/// `device_board_size` with runtime info recording `runtime` and OTP
/// containing `otp`. Returns the size and whether OTP was read.
async fn size_with(
    runtime: Option<MaybeKnown<OneromBoardSize>>,
    otp: Option<OneromBoardSize>,
) -> (Option<MaybeKnown<OneromBoardSize>>, bool) {
    let read = Cell::new(false);
    let size = device_board_size(runtime, || async {
        read.set(true);
        otp
    })
    .await;
    (size, read.get())
}

#[tokio::test]
async fn a_size_runtime_info_records_is_used_without_reading_otp() {
    for size in [
        MaybeKnown::Known(BoardSizeM),
        MaybeKnown::Known(BoardSizeL),
        MaybeKnown::Known(BoardSizeOther),
        MaybeKnown::Unknown(0x37),
    ] {
        assert_eq!(
            size_with(Some(size), Some(BoardSizeM)).await,
            (Some(size), false),
            "{size:?}"
        );
    }
}

/// Firmware before 0.8.0 records `BoardSizeUnknown`, and a stopped One ROM
/// doesn't have runtime info.
#[tokio::test]
async fn otp_is_read_where_runtime_info_doesnt_record_a_size() {
    for runtime in [Some(MaybeKnown::Known(BoardSizeUnknown)), None] {
        for otp in [BoardSizeM, BoardSizeL, BoardSizeOther] {
            assert_eq!(
                size_with(runtime, Some(otp)).await,
                (Some(MaybeKnown::Known(otp)), true),
                "{runtime:?} {otp:?}"
            );
        }
    }
}

#[tokio::test]
async fn a_failed_otp_read_leaves_the_size_runtime_info_has() {
    for runtime in [Some(MaybeKnown::Known(BoardSizeUnknown)), None] {
        assert_eq!(
            size_with(runtime, None).await,
            (runtime, true),
            "{runtime:?}"
        );
    }
}

#[test]
fn m_and_l_are_their_own_sizes() {
    assert_eq!(
        board_size_or_m(Some(MaybeKnown::Known(BoardSizeM))),
        BoardSize::M
    );
    assert_eq!(
        board_size_or_m(Some(MaybeKnown::Known(BoardSizeL))),
        BoardSize::L
    );
}

/// Every board has the first flash chip.
#[test]
fn any_other_size_is_m() {
    for size in [
        Some(MaybeKnown::Known(BoardSizeUnknown)),
        Some(MaybeKnown::Known(BoardSizeOther)),
        Some(MaybeKnown::Unknown(0x37)),
        None,
    ] {
        assert_eq!(board_size_or_m(size), BoardSize::M, "{size:?}");
    }
}
