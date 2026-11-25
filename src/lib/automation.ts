// Hooks for the UI test scripts in scripts/dev. They are switched on only when the Rust
// side was built with the `automation` feature (never in release builds), so a normal
// installation exposes neither the store nor the file-dialog stand-in.

let enabled = false;

export const automationEnabled = () => enabled;

export function enableAutomation(store: unknown) {
  enabled = true;
  (window as unknown as { __CN_STORE__: unknown }).__CN_STORE__ = store;
}
