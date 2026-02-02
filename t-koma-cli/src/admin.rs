//! Admin mode for managing user approvals (interactive).

use std::io::{self, Write};

use t_koma_core::{ApprovedUser, PendingUser, PendingUsers, PersistentConfig};

/// Run the admin interactive mode
pub async fn run_admin_mode() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n╔════════════════════════════════════╗");
    println!("║        t-koma Admin Mode           ║");
    println!("╚════════════════════════════════════╝\n");

    loop {
        // Load fresh data
        let mut config = match PersistentConfig::load() {
            Ok(c) => c,
            Err(e) => {
                println!("Error loading config: {}. Creating new...", e);
                PersistentConfig::default()
            }
        };

        let mut pending = match PendingUsers::load() {
            Ok(p) => p,
            Err(e) => {
                println!("Error loading pending users: {}. Creating new...", e);
                PendingUsers::default()
            }
        };

        // Show pending Discord users
        let pending_list: Vec<PendingUser> = pending
            .list()
            .into_iter()
            .cloned()
            .collect();
        
        if pending_list.is_empty() {
            println!("No pending users waiting for approval.");
            println!("\nCommands: [r]efresh, [l]ist approved, [q]uit");
        } else {
            println!("\n=== Pending Discord Users ===\n");
            
            for (i, user) in pending_list.iter().enumerate() {
                let mins_ago = chrono::Utc::now()
                    .signed_duration_since(user.requested_at)
                    .num_minutes();
                println!(
                    "  {}. @{} (ID: {}) - {} min ago",
                    i + 1,
                    user.name,
                    user.id,
                    mins_ago
                );
            }

            println!("\nEnter number to approve, 'd <num>' to deny, 'r' to refresh, 'l' to list approved, 'q' to quit");
        }

        print!("\nadmin> ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        // Parse command
        let parts: Vec<&str> = input.split_whitespace().collect();
        let cmd = parts[0];

        match cmd {
            "q" | "quit" | "exit" => {
                println!("Goodbye!");
                return Ok(());
            }
            "r" | "refresh" => {
                println!("Refreshing...");
                continue;
            }
            "l" | "list" => {
                list_approved(&config);
                continue;
            }
            "d" | "deny" => {
                if parts.len() < 2 {
                    println!("Usage: d <number>");
                    continue;
                }
                
                let num: usize = match parts[1].parse() {
                    Ok(n) if n > 0 && n <= pending_list.len() => n,
                    _ => {
                        println!("Invalid number. Use 1-{}.", pending_list.len());
                        continue;
                    }
                };

                let user = &pending_list[num - 1];
                let user_id = user.id.clone();
                let user_name = user.name.clone();
                
                // Remove from pending (don't add to approved)
                pending.remove(&user_id);
                if let Err(e) = pending.save() {
                    println!("Error saving pending: {}", e);
                } else {
                    println!("✗ Denied @{} (ID: {})", user_name, user_id);
                }
            }
            _ => {
                // Try to parse as a number for approval
                let num: usize = match cmd.parse() {
                    Ok(n) if n > 0 && n <= pending_list.len() => n,
                    _ => {
                        println!("Unknown command: {}", cmd);
                        println!("Enter a number to approve, 'd <num>' to deny, 'q' to quit.");
                        continue;
                    }
                };

                let user = &pending_list[num - 1];
                let user_id = user.id.clone();
                let user_name = user.name.clone();
                
                // Remove from pending
                pending.remove(&user_id);
                if let Err(e) = pending.save() {
                    println!("Error saving pending: {}", e);
                    continue;
                }
                
                // Add to approved
                config.discord.add(&user_id, &user_name);
                if let Err(e) = config.save() {
                    println!("Error saving config: {}", e);
                    continue;
                }

                println!("✓ Approved @{} (ID: {})", user_name, user_id);
                println!("  They'll receive a welcome message on their next interaction.");
            }
        }
    }
}

/// List approved Discord users
fn list_approved(config: &PersistentConfig) {
    let users: Vec<&ApprovedUser> = config.discord.list();

    if users.is_empty() {
        println!("\nNo approved users.");
        return;
    }

    println!("\n=== Approved Discord Users ===\n");
    println!("{:<20} {:<20} {:<12} Approved At", "Name", "ID", "Welcomed");
    println!("{:-<70}", "");

    for user in users {
        let approved_str = user.approved_at.format("%Y-%m-%d %H:%M");
        let welcomed = if user.welcomed { "yes" } else { "no" };
        println!(
            "{:<20} {:<20} {:<12} {}",
            user.name, user.id, welcomed, approved_str
        );
    }
}
