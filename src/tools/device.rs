use super::{Tool, ToolResult, arg_i64, arg_string};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::process::Command;
pub struct I2cTool;
pub struct SpiTool;
#[async_trait]
impl Tool for I2cTool {
    fn name(&self) -> &str {
        "i2c"
    }
    fn description(&self) -> &str {
        "Interact with I2C bus devices (Linux only)"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["detect", "scan", "read", "write"] },
                "bus": { "type": "string" },
                "address": { "type": "integer" },
                "register": { "type": "integer" },
                "data": { "type": "array", "items": { "type": "integer" } },
                "length": { "type": "integer" },
                "confirm": { "type": "boolean" }
            },
            "required": ["action"]
        })
    }
    async fn execute(&self, args: HashMap<String, Value>, _: &str, _: &str) -> ToolResult {
        if !cfg!(target_os = "linux") {
            return ToolResult::error("I2C is only supported on Linux.");
        }
        let action = match arg_string(&args, "action") {
            Some(v) => v,
            None => return ToolResult::error("action is required"),
        };
        match action.as_str() {
            "detect" => {
                let mut buses = Vec::new();
                if let Ok(paths) = glob::glob("/dev/i2c-*") {
                    for p in paths.flatten() {
                        buses.push(p.display().to_string());
                    }
                }
                if buses.is_empty() {
                    return ToolResult {
                        for_user: None,
                        for_llm: Some("No I2C buses found".into()),
                        silent: true,
                        error: None,
                    };
                }
                let payload = serde_json::to_string_pretty(&buses).unwrap_or_default();
                ToolResult {
                    for_user: None,
                    for_llm: Some(format!("Found {} I2C bus(es):\n{}", buses.len(), payload)),
                    silent: true,
                    error: None,
                }
            }
            "scan" => {
                let bus = match arg_string(&args, "bus") {
                    Some(v) if !v.is_empty() => v,
                    _ => return ToolResult::error("bus is required"),
                };
                let dev = format!("/dev/i2c-{}", bus);
                if !std::path::Path::new(&dev).exists() {
                    return ToolResult::error(&format!("I2C bus not found: {}", dev));
                }
                match Command::new("i2cdetect").args(["-y", &bus]).output() {
                    Ok(out) if out.status.success() => ToolResult {
                        for_user: None,
                        for_llm: Some(String::from_utf8_lossy(&out.stdout).to_string()),
                        silent: true,
                        error: None,
                    },
                    Ok(out) => ToolResult::error(&String::from_utf8_lossy(&out.stderr)),
                    Err(_) => ToolResult::error("i2cdetect command not available"),
                }
            }
            "read" => {
                let bus = match arg_string(&args, "bus") {
                    Some(v) if !v.is_empty() => v,
                    _ => return ToolResult::error("bus is required"),
                };
                let addr = match arg_i64(&args, "address") {
                    Some(v) if (0x03..=0x77).contains(&v) => v as u8,
                    _ => return ToolResult::error("address must be 0x03-0x77"),
                };
                let reg = arg_i64(&args, "register");
                let output = if let Some(r) = reg {
                    Command::new("i2cget")
                        .args([
                            "-y",
                            &bus,
                            &format!("0x{addr:02x}"),
                            &format!("0x{:02x}", r as u8),
                        ])
                        .output()
                } else {
                    Command::new("i2cget")
                        .args(["-y", &bus, &format!("0x{addr:02x}")])
                        .output()
                };
                match output {
                    Ok(out) if out.status.success() => ToolResult {
                        for_user: None,
                        for_llm: Some(format!(
                            "Read: {}",
                            String::from_utf8_lossy(&out.stdout).trim()
                        )),
                        silent: true,
                        error: None,
                    },
                    Ok(out) => ToolResult::error(&String::from_utf8_lossy(&out.stderr)),
                    Err(_) => ToolResult::error("i2cget not available"),
                }
            }
            "write" => {
                if args.get("confirm").and_then(|v| v.as_bool()) != Some(true) {
                    return ToolResult::error("confirm=true required");
                }
                let bus = match arg_string(&args, "bus") {
                    Some(v) if !v.is_empty() => v,
                    _ => return ToolResult::error("bus is required"),
                };
                let addr = match arg_i64(&args, "address") {
                    Some(v) if (0x03..=0x77).contains(&v) => v as u8,
                    _ => return ToolResult::error("address must be 0x03-0x77"),
                };
                let data = match args.get("data").and_then(|v| v.as_array()) {
                    Some(v) if !v.is_empty() => v,
                    _ => return ToolResult::error("data required"),
                };
                let reg = arg_i64(&args, "register");
                let mut cmd = Command::new("i2cset");
                cmd.args(["-y", &bus, &format!("0x{addr:02x}")]);
                if let Some(r) = reg {
                    cmd.arg(format!("0x{:02x}", r as u8));
                }
                for b in data {
                    match b.as_i64() {
                        Some(n) if (0..=255).contains(&n) => {
                            cmd.arg(format!("0x{:02x}", n as u8));
                        }
                        _ => return ToolResult::error("data bytes must be 0..255"),
                    }
                }
                match cmd.output() {
                    Ok(out) if out.status.success() => ToolResult {
                        for_user: None,
                        for_llm: Some("I2C write completed".into()),
                        silent: true,
                        error: None,
                    },
                    Ok(out) => ToolResult::error(&String::from_utf8_lossy(&out.stderr)),
                    Err(_) => ToolResult::error("i2cset not available"),
                }
            }
            _ => ToolResult::error("unknown action"),
        }
    }
}
#[async_trait]
impl Tool for SpiTool {
    fn name(&self) -> &str {
        "spi"
    }
    fn description(&self) -> &str {
        "Interact with SPI bus devices (Linux only)"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["list", "transfer", "read"] },
                "device": { "type": "string" },
                "data": { "type": "array", "items": { "type": "integer" } },
                "length": { "type": "integer" },
                "confirm": { "type": "boolean" }
            },
            "required": ["action"]
        })
    }
    async fn execute(&self, args: HashMap<String, Value>, _: &str, _: &str) -> ToolResult {
        if !cfg!(target_os = "linux") {
            return ToolResult::error("SPI is only supported on Linux.");
        }
        let action = match arg_string(&args, "action") {
            Some(v) => v,
            None => return ToolResult::error("action is required"),
        };
        match action.as_str() {
            "list" => {
                let mut devs = Vec::new();
                if let Ok(paths) = glob::glob("/dev/spidev*") {
                    for p in paths.flatten() {
                        devs.push(p.display().to_string());
                    }
                }
                if devs.is_empty() {
                    return ToolResult {
                        for_user: None,
                        for_llm: Some("No SPI devices found".into()),
                        silent: true,
                        error: None,
                    };
                }
                let payload = serde_json::to_string_pretty(&devs).unwrap_or_default();
                ToolResult {
                    for_user: None,
                    for_llm: Some(format!("Found {} SPI device(s):\n{}", devs.len(), payload)),
                    silent: true,
                    error: None,
                }
            }
            "transfer" => {
                if args.get("confirm").and_then(|v| v.as_bool()) != Some(true) {
                    return ToolResult::error("confirm=true required");
                }
                let device = match arg_string(&args, "device") {
                    Some(v) if !v.is_empty() => v,
                    _ => return ToolResult::error("device required"),
                };
                let data = match args.get("data").and_then(|v| v.as_array()) {
                    Some(v) if !v.is_empty() => v,
                    _ => return ToolResult::error("data required"),
                };
                let hex: Vec<String> = data
                    .iter()
                    .filter_map(|v| v.as_i64())
                    .map(|n| format!("{:02x}", n.clamp(0, 255) as u8))
                    .collect();
                if hex.is_empty() {
                    return ToolResult::error("data bytes must be integers");
                }
                match Command::new("spidev_test")
                    .args(["-D", &format!("/dev/spidev{}", device), "-p", &hex.join("")])
                    .output()
                {
                    Ok(out) if out.status.success() => ToolResult {
                        for_user: None,
                        for_llm: Some(String::from_utf8_lossy(&out.stdout).to_string()),
                        silent: true,
                        error: None,
                    },
                    Ok(out) => ToolResult::error(&String::from_utf8_lossy(&out.stderr)),
                    Err(_) => ToolResult::error("spidev_test not available"),
                }
            }
            "read" => {
                let device = match arg_string(&args, "device") {
                    Some(v) if !v.is_empty() => v,
                    _ => return ToolResult::error("device required"),
                };
                let length = arg_i64(&args, "length").unwrap_or(1).clamp(1, 4096) as usize;
                let zeros = "00".repeat(length);
                match Command::new("spidev_test")
                    .args(["-D", &format!("/dev/spidev{}", device), "-p", &zeros])
                    .output()
                {
                    Ok(out) if out.status.success() => ToolResult {
                        for_user: None,
                        for_llm: Some(String::from_utf8_lossy(&out.stdout).to_string()),
                        silent: true,
                        error: None,
                    },
                    Ok(out) => ToolResult::error(&String::from_utf8_lossy(&out.stderr)),
                    Err(_) => ToolResult::error("spidev_test not available"),
                }
            }
            _ => ToolResult::error("unknown action"),
        }
    }
}
