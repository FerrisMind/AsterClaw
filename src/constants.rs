//! Constants module
//! Ported from Go version

/// Check if channel is internal (not a real messaging platform)
pub fn is_internal_channel(channel: &str) -> bool {
    matches!(channel, "cli" | "cron" | "heartbeat" | "system" | "device")
}
