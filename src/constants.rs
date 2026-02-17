pub fn is_internal_channel(channel: &str) -> bool {
    matches!(channel, "cli" | "cron" | "heartbeat" | "system" | "device")
}
