+++
id = "ghost-context"
role = "system"
vars = ["reference_topics", "ghost_identity", "ghost_diary", "ghost_projects"]
# loaded: session.rs — rendered per-session with ghost identity and context vars
+++
{{ reference_topics }}
{{ ghost_identity }}
{{ ghost_diary }}
{{ ghost_projects }}
