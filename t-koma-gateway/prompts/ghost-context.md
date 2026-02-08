+++
id = "ghost-context"
role = "system"
vars = ["ghost_identity", "ghost_diary", "ghost_skills", "system_info"]
# loaded: session.rs — rendered per-session with ghost identity and context vars
+++
{{ system_info }}
{{ ghost_identity }}
{{ ghost_diary }}
{{ ghost_skills }}
