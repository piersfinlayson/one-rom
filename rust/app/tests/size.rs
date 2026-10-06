// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for where a device's board size comes from and which sizes are known.

use core::cell::Cell;

use onerom_app::{BoardSize, device_board_size, known_board_size};
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
fn only_m_and_l_are_known_sizes() {
    let m = known_board_size(Some(MaybeKnown::Known(BoardSizeM)));
    assert_eq!(m, Some(BoardSize::M));
    let l = known_board_size(Some(MaybeKnown::Known(BoardSizeL)));
    assert_eq!(l, Some(BoardSize::L));
    for size in [
        Some(MaybeKnown::Known(BoardSizeUnknown)),
        Some(MaybeKnown::Known(BoardSizeOther)),
        Some(MaybeKnown::Unknown(0x37)),
        None,
    ] {
        assert_eq!(known_board_size(size), None, "{size:?}");
    }
}
