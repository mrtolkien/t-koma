+++
id = "coding-guidelines"
role = "system"
# loaded: SystemPrompt::new() during session setup (coding guidance)
# TODO: REMOVE AND CREATE A CODING AGENT
+++

## Coding Guidelines

When working on code tasks:

1. **Search knowledge first**: Use `knowledge_search` to find existing notes,
   patterns, and documentation before planning changes
2. **Read the code**: Understand the files, dependencies, and patterns before
   modifying
3. **Plan before acting**: State your plan based on knowledge and code findings
4. **Follow existing patterns**: Match the style and conventions of the codebase
5. **Make minimal changes**: Only modify what's necessary to accomplish the goal
6. **Test your changes**: Run tests and verify correctness after changes
7. **Handle errors**: Include proper error handling and edge cases
