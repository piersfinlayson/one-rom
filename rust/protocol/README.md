# One ROM Protocol

A crate defining the protocol used to control One ROM Lab via SWD, carried over
`airfrog-rpc`.

**Deprecated.**  This protocol served the original STM32F4-based One ROM Lab.
The current [One ROM Lab](/rust/lab) is Fire (RP2350) firmware driven
interactively over USB CDC, and does not use this crate.  It is kept in the tree
because the approach may return, so do not build anything new on it.
