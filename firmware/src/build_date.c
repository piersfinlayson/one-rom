// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// When this firmware was built, as UTC.
//
// This lives on its own because the Makefile makes this object depend on every
// other one, so it is recompiled whenever anything else was, and the stamp is
// the moment the firmware was actually built.  Held anywhere else it would say
// when that file last happened to compile, which can be any age at all.
//
// ONEROM_BUILD_DATE comes from that same rule.  The fallback is the compiler's
// own local time, with no zone recorded, and is there so a build outside the
// Makefile still compiles.

#include "include.h"

#if !defined(ONEROM_BUILD_DATE)
#define ONEROM_BUILD_DATE __DATE__ " " __TIME__
#endif

const char onerom_build_date[] = ONEROM_BUILD_DATE;
