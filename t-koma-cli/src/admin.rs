//! Admin mode for managing operator approvals (interactive).

use std::io::{self, Write};

use chrono::{TimeZone, Utc};
use t_koma_db::{KomaDbPool, OperatorRepository, OperatorStatus, Platform};

/// Run the admin interactive mode
pub async fn run_admin_mode() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n╔════════════════════════════════════╗");
    println!("║        t-koma Admin Mode           ║");
    println!("╚════════════════════════════════════╝\n");

    // Initialize database
    let db = KomaDbPool::new().await?;
    println!("Connected to database.\n");

    loop {
        // Prune old pending operators before showing list
        match OperatorRepository::prune_pending(db.pool(), 1).await {
            Ok(count) => {
                if count > 0 {
                    println!("Pruned {} expired pending operators.", count);
                }
            }
            Err(e) => {
                eprintln!("Warning: failed to prune pending operators: {}", e);
            }
        }

        // Show pending operators
        let pending_list = match OperatorRepository::list_by_status(
            db.pool(),
            OperatorStatus::Pending,
            None,
        )
        .await
        {
            Ok(list) => list,
            Err(e) => {
                eprintln!("Error loading pending operators: {}", e);
                Vec::new()
            }
        };

        if pending_list.is_empty() {
            println!("No pending operators waiting for approval.");
            println!("\nCommands: [r]efresh, [l]ist approved, [d]enied list, [q]uit");
        } else {
            println!("\n=== Pending Operators ===\n");

            for (i, operator) in pending_list.iter().enumerate() {
                let created_at = Utc
                    .timestamp_opt(operator.created_at, 0)
                    .single()
                    .unwrap_or_else(|| Utc.timestamp_opt(0, 0).unwrap());
                let mins_ago = Utc::now().signed_duration_since(created_at).num_minutes();
                let platform = format!("{:?}", operator.platform).to_lowercase();
                println!(
                    "  {}. @{} [{}] (ID: {}) - {} min ago",
                    i + 1,
                    operator.name,
                    platform,
                    operator.id,
                    mins_ago
                );
            }

            println!(
                "\nEnter number to approve, 'd <num>' to deny, 'r' to refresh, 'l' to list approved, 'D' for denied list, 'q' to quit"
            );
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
                list_by_status(&db, OperatorStatus::Approved, "Approved").await;
                continue;
            }
            "D" | "denied-list" => {
                list_by_status(&db, OperatorStatus::Denied, "Denied").await;
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

                let operator = &pending_list[num - 1];
                let operator_id = operator.id.clone();
                let operator_name = operator.name.clone();

                // Deny operator
                match OperatorRepository::deny(db.pool(), &operator_id).await {
                    Ok(_) => {
                        println!("✗ Denied @{} (ID: {})", operator_name, operator_id);
                    }
                    Err(e) => {
                        println!("Error denying operator: {}", e);
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

                let operator = &pending_list[num - 1];
                let operator_id = operator.id.clone();
                let operator_name = operator.name.clone();

                // Approve operator
                match OperatorRepository::approve(db.pool(), &operator_id).await {
                    Ok(_) => {
                        println!("✓ Approved @{} (ID: {})", operator_name, operator_id);
                        if operator.platform == Platform::Discord {
                            println!(
                                "  They'll receive a welcome message on their next interaction."
                            );
                        }
                    }
                    Err(e) => {
                        println!("Error approving operator: {}", e);
                    }
                }
            }
        }
    }
}

/// List users by status
async fn list_by_status(db: &KomaDbPool, status: OperatorStatus, label: &str) {
    let operators = match OperatorRepository::list_by_status(db.pool(), status, None).await {
        Ok(list) => list,
        Err(e) => {
            eprintln!("Error loading operators: {}", e);
            return;
        }
    };

    if operators.is_empty() {
        println!("\nNo {} operators.", label.to_lowercase());
        return;
    }

    println!("\n=== {} Operators ===\n", label);
    println!(
        "{:<20} {:<12} {:<20} {:<12} Updated At",
        "Name", "Platform", "ID", "Welcomed"
    );
    println!("{:-<80}", "");

    for operator in operators {
        let updated_at = Utc
            .timestamp_opt(operator.updated_at, 0)
            .single()
            .unwrap_or_else(|| Utc.timestamp_opt(0, 0).unwrap());
        let updated_str = updated_at.format("%Y-%m-%d %H:%M");
        let platform = format!("{:?}", operator.platform).to_lowercase();
        let welcomed = if operator.welcomed { "yes" } else { "no" };
        println!(
            "{:<20} {:<12} {:<20} {:<12} {}",
            operator.name, platform, operator.id, welcomed, updated_str
        );
    }
}
