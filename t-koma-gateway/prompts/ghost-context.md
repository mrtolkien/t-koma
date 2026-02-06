+++
id = "ghost-context"
role = "system"
vars = ["reference_topics", "ghost_identity", "ghost_diary", "ghost_projects", "system_info"]
# loaded: session.rs — rendered per-session with ghost identity and context vars
+++
{{ system_info }}
{{ reference_topics }}
{{ ghost_identity }}
{{ ghost_diary }}
{{ ghost_projects }}
