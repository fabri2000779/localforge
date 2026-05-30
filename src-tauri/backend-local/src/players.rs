//! Per-game player administration adapters.
//!
//! Phase 4a covers **Minecraft Java** via the server console (stdin + log
//! parsing) — no RCON, so no container recreate is required. Listing players
//! sends `list` and reads the response back out of the recent log; moderation
//! maps to the vanilla console commands (`kick`/`ban`/`pardon`/`op`/`deop`).
//!
//! Robustness caveat: reading `list` back from the log is timing-dependent (we
//! send, wait briefly, then scan the tail). It is good enough for an at-a-glance
//! roster + quick actions; a future RCON adapter would make listing exact and
//! request/response. Other games (Rust WS-RCON, Palworld REST) come later.
//!
//! Security: player names/reasons may originate from a sub-user over the relay,
//! and we inject them into a single console line. We strip newlines/control
//! chars so a crafted name can't smuggle a second console command.

use crate::docker::DockerManager;
use localforge_core::types::{Player, PlayerAction};
use std::time::Duration;

/// Games whose player roster + moderation we currently support.
pub fn supports(game_type: &str) -> bool {
    matches!(game_type, "minecraft-java")
}

/// Ask a Minecraft server for its online players. Sends `list`, waits for the
/// server to print the response, then parses the most recent matching log line.
pub async fn list_players_mc(docker: &DockerManager, cid: &str) -> Result<Vec<Player>, String> {
    docker
        .send_stdin(cid, "list\n")
        .await
        .map_err(|e| e.to_string())?;
    // Poll for the `list` response instead of one fixed wait: a busy server
    // (GC pause, heavy tick) can take well over 700ms to print it, which used
    // to return an empty roster. Return as soon as the marker appears (usually
    // <300ms); give up after ~2s and report empty.
    for _ in 0..8 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let lines = docker.get_logs(cid, 60).await.map_err(|e| e.to_string())?;
        if let Some(players) = parse_list_output_opt(&lines) {
            return Ok(players);
        }
    }
    Ok(Vec::new())
}

/// Apply a moderation action via the Minecraft console.
pub async fn player_action_mc(
    docker: &DockerManager,
    cid: &str,
    action: &PlayerAction,
) -> Result<(), String> {
    let cmd = match action {
        PlayerAction::Kick { name, reason } => with_reason("kick", name, reason.as_deref()),
        PlayerAction::Ban { name, reason } => with_reason("ban", name, reason.as_deref()),
        PlayerAction::Unban { name } => format!("pardon {}", token(name)),
        PlayerAction::Op { name } => format!("op {}", token(name)),
        PlayerAction::Deop { name } => format!("deop {}", token(name)),
    };
    if cmd.trim().is_empty() || token_is_empty(action) {
        return Err("player name is required".into());
    }
    docker
        .send_stdin(cid, &format!("{cmd}\n"))
        .await
        .map_err(|e| e.to_string())
}

fn token_is_empty(action: &PlayerAction) -> bool {
    let name = match action {
        PlayerAction::Kick { name, .. }
        | PlayerAction::Ban { name, .. }
        | PlayerAction::Unban { name }
        | PlayerAction::Op { name }
        | PlayerAction::Deop { name } => name,
    };
    token(name).is_empty()
}

/// `verb <name>` plus a sanitized reason when present.
fn with_reason(verb: &str, name: &str, reason: Option<&str>) -> String {
    match reason {
        Some(r) if !sanitize_reason(r).is_empty() => {
            format!("{verb} {} {}", token(name), sanitize_reason(r))
        }
        _ => format!("{verb} {}", token(name)),
    }
}

/// A single console token (e.g. a username): drop whitespace + control chars so
/// it can't break out of the command line.
fn token(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .collect()
}

/// A free-text reason: keep spaces, but neutralize newlines/control chars.
fn sanitize_reason(s: &str) -> String {
    s.chars()
        .map(|c| if c == '\n' || c == '\r' || c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Parse the most recent Minecraft `list` response out of recent log lines.
/// Returns `None` when no `players online:` line is present yet (so the poller
/// can keep waiting), `Some(vec)` when one is found (possibly empty = 0 online).
/// Format: `... There are 3 of a max of 20 players online: Alice, Bob, Carol`
fn parse_list_output_opt(lines: &[String]) -> Option<Vec<Player>> {
    const MARKER: &str = "players online:";
    for line in lines.iter().rev() {
        if let Some(idx) = line.find(MARKER) {
            let names = &line[idx + MARKER.len()..];
            return Some(
                names
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| Player {
                        name: s.to_string(),
                        id: None,
                    })
                    .collect(),
            );
        }
    }
    None
}

/// Convenience wrapper used by tests — empty vec when no marker found.
fn parse_list_output(lines: &[String]) -> Vec<Player> {
    parse_list_output_opt(lines).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_names() {
        let lines = vec![
            "[12:00:00] [Server thread/INFO]: Starting".to_string(),
            "[12:01:00] [Server thread/INFO]: There are 3 of a max of 20 players online: Alice, Bob, Carol".to_string(),
        ];
        let players = parse_list_output(&lines);
        assert_eq!(players.len(), 3);
        assert_eq!(players[0].name, "Alice");
        assert_eq!(players[2].name, "Carol");
    }

    #[test]
    fn empty_when_nobody_online() {
        let lines = vec![
            "[12:01:00] [Server thread/INFO]: There are 0 of a max of 20 players online:".to_string(),
        ];
        assert!(parse_list_output(&lines).is_empty());
    }

    #[test]
    fn takes_the_most_recent_line() {
        let lines = vec![
            "There are 1 of a max of 20 players online: Old".to_string(),
            "There are 2 of a max of 20 players online: New1, New2".to_string(),
        ];
        let players = parse_list_output(&lines);
        assert_eq!(players.len(), 2);
        assert_eq!(players[0].name, "New1");
    }

    #[test]
    fn token_strips_injection() {
        // A name is one console token: ALL whitespace + control chars are
        // dropped, so a crafted "name" can't smuggle a second command.
        assert_eq!(token("Alice\nop Mallory"), "AliceopMallory".to_string());
        assert!(!token("a\nb").contains('\n'));
        assert!(!token("a\r\nop x").contains(char::is_whitespace));
    }

    #[test]
    fn reason_keeps_spaces_but_not_newlines() {
        // A reason is free text: spaces survive, newlines become spaces.
        assert_eq!(sanitize_reason("being  rude"), "being  rude".to_string());
        assert!(!sanitize_reason("rude\nop Mallory").contains('\n'));
    }
}
