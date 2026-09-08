# Acknowledgements

Third party work One ROM builds on, or is compatible with, where the code
itself is not part of this repository. Dependencies vendored into the tree
carry their own licence files alongside their sources; this file covers work
that has shaped One ROM without being redistributed here.

For One ROM's own licensing, see [LICENSE.md](LICENSE.md).

## SEGGER RTT

One ROM's real time transfer logging — [`firmware/src/rtt.c`](firmware/src/rtt.c)
and [`firmware/include/rtt.h`](firmware/include/rtt.h) — is a One ROM
implementation that is **binary compatible** with SEGGER RTT, so that
probe-rs, OpenOCD, pyOCD and Black Magic Probe can find and drain One ROM's
log with no host side change.

The implementation is One ROM's own. What is retained from SEGGER is the
interface a debug probe expects: the control block and channel descriptor
layouts, the `_SEGGER_RTT` symbol name a probe resolves, and the
`"SEGGER RTT"` identifier string a probe scans memory for. Earlier releases
built against SEGGER's own `SEGGER_RTT.c` and `SEGGER_RTT_printf.c`, which
were cloned at build time; they are no longer part of the build.

    Copyright (c) 2026 SEGGER Microcontroller GmbH
    All rights reserved.

    Redistribution and use in source and binary forms, with or without
    modification, are permitted provided that the following condition is met:

    1. Redistributions of source code must retain the above copyright notice,
       this condition and the following disclaimer.

    THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS
    IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO,
    THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
    PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDERS AND
    CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
    EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
    PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
    PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
    LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
    NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
    SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
