//! Admin mode for managing user approvals (interactive).

use std::io::{self, Write};

use t_koma_db::{DbPool, Platform, UserRepository, UserStatus};

/// Run the admin interactive mode
pub async fn run_admin_mode() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n╔════════════════════════════════════╗");
    println!("║        t-koma Admin Mode           ║");
    println!("╚════════════════════════════════════╝\n");

    // Initialize database
    let db = DbPool::new().await?;
    println!("Connected to database.\n");

    loop {
        // Prune old pending users before showing list
        match UserRepository::prune_pending(db.pool(), 1).await {
            Ok(count) => {
                if count > 0 {
                    println!("Pruned {} expired pending users.", count);
                }
            }
            Err(e) => {
                eprintln!("Warning: failed to prune pending users: {}", e);
            }
        }

        // Show pending users
        let pending_list = match UserRepository::list_by_status(db.pool(), UserStatus::Pending, None).await {
            Ok(list) => list,
            Err(e) => {
                eprintln!("Error loading pending users: {}", e);
                Vec::new()
            }
        };

        if pending_list.is_empty() {
            println!("No pending users waiting for approval.");
            println!("\nCommands: [r]efresh, [l]ist approved, [d]enied list, [q]uit");
        } else {
            println!("\n=== Pending Users ===\n");

            for (i, user) in pending_list.iter().enumerate() {
                let mins_ago = chrono::Utc::now()
                    .signed_duration_since(user.created_at)
                    .num_minutes();
                let platform = format!("{:?}", user.platform).to_lowercase();
                println!(
                    "  {}. @{} [{}] (ID: {}) - {} min ago",
                    i + 1,
                    user.name,
                    platform,
                    user.id,
                    mins_ago
                );
            }

            println!("\nEnter number to approve, 'd <num>' to deny, 'r' to refresh, 'l' to list approved, 'D' for denied list, 'q' to quit");
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
                list_by_status(&db, UserStatus::Approved, "Approved").await;
                continue;
            }
            "D" | "denied-list" => {
                list_by_status(&db, UserStatus::Denied, "Denied").await;
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

                // Deny user
                match UserRepository::deny(db.pool(), &user_id).await {
                    Ok(_) => {
                        println!("✗ Denied @{} (ID: {})", user_name, user_id);
                    }
                    Err(e) => {
                        println!("Error denying user: {}", e);
                    }
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

                // Approve user
                match UserRepository::approve(db.pool(), &user_id).await {
                    Ok(_) => {
                        println!("✓ Approved @{} (ID: {})", user_name, user_id);
                        if user.platform == Platform::Discord {
                            println!("  They'll receive a welcome message on their next interaction.");
                        }
                    }
                    Err(e) => {
                        println!("Error approving user: {}", e);
                    }
                }
            }
        }
    }
}

/// List users by status
async fn list_by_status(db: &DbPool, status: UserStatus, label: &str) {
    let users = match UserRepository::list_by_status(db.pool(), status, None).await {
        Ok(list) => list,
        Err(e) => {
            eprintln!("Error loading users: {}", e);
            return;
        }
    };

    if users.is_empty() {
        println!("\nNo {} users.", label.to_lowercase());
        return;
    }

    println!("\n=== {} Users ===\n", label);
    println!("{:<20} {:<12} {:<20} {:<12} Updated At", "Name", "Platform", "ID", "Welcomed");
    println!("{:-<80}", "");

    for user in users {
        let updated_str = user.updated_at.format("%Y-%m-%d %H:%M");
        let platform = format!("{:?}", user.platform).to_lowercase();
        let welcomed = if user.welcomed { "yes" } else { "no" };
        println!(
            "{:<20} {:<12} {:<20} {:<12} {}",
            user.name, platform, user.id, welcomed, updated_str
        );
    }
}
