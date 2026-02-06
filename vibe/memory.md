# T-Koma knowledge system

Read AGENTS.md and get ready for an extremely complex feature: the ghosts's
memory and the t-koma knowledge system.

The goal will be to provide the ghosts with long-term data retention systems.
Aka its memory. And the t-koma itself with a knowledge base shared by all
ghosts.

Organize the code in its own crate, with only the tools as the public API for
the gateway.

Here is how the t-koma knowledge system will be organized:

- In the `xdg_data/knowledge` folder, there will be Markdown notes with TOML
  front-matter
- Those files will contain OBJECTIVE FACTS AND KNOWLEDGE gathered from the
  ghosts. Things like products specifications, public information about
  organizations or places, knowledge about recent events/news, how-to for things
  that ghosts failed to realize directly in the past, self-written
  documentation, ...
- The front-matter will contain the following information:
  - When the data was created and by whom (ghost + model)
  - When it was last validated and by whom
  - How trustworthy it on a 1 to 10 scale
  - Ghosts can also leave _comments_ on the content
  - An optional PARENT note -> if it has one, it will be in a sub-folder under
    the parent
  - Anything else you deem useful
- The file's body will be in Markdown, and its content will be embedded in the
  gateway's sqlite-vec database for querying. The metadata and links will be
  saved in the database as well for graph-based RAG (getting parent/links)
- It will be possible to tag other notes with Obsidian format: `[[NOTE_NAME]]`.
  Links can be created before the note exist, but then the note will exist in
  the knowledge graph. The ghosts should make extensive use of link for pages
  like categories.
  - Example: the ratatui.md file should link to `CLI libraries`, to
    `Rust libraries`, to `Used internally`, ...
- File titles should be short and descriptive, and the first paragraph should
  explain what the note _is_. Its attributes.
- In the `xdg_data/reference`, there will be _reference_ information like
  libraries documentation, source code, ...

Then, the ghost will have its own memory:

- In `workspace/projects`, it will manage multiple active projects. They will
  each have a README.md and the first paragraph should explain the topic, and
  will always be in the context.
  - Projects could be planning holidays, writing new code, doing a product
    comparison, ...
  - Projects can be archived to `projects/.archive` if too old or finished. We
    will have a special "Project done" flow that reviews crucial project
    information before archiving it.
- In `workspace/diary`, there will be one file per day where the ghost was
  talked to, recapping what happened on that day. The diary of the past 2 days
  will be loaded in the prompt on new conversations.
- In `workspace/private_knowledge`, there will be knowledge structured like the
  shared one, but with sensitive information. It should still be possible to
  link out to shared knowledge.
- The `workspace/SOUL.md,USER.md,BOOT.md` are three files that are _always
  loaded in the prompt_:
  - The boot, which will be the ghost-specific system prompt holding long-term
    crucial memory on how it should behave
  - The soul, which the ghost is allowed to write to when it experiences things
  - The user, which is the ghosts' information key information about the user

The knowledge system will be made available through tools:

- `memory_search` will search both the ghosts' memory and the knowledge base
  through embeddings, BM25 search, and graph traversal
  - Embeddings and BM25 will give lists of results, we will also provide the
    parents, tags, and first-depth links (both incoming and outgoing)
  - There should be options to the tools (with simple defaults) to allow for
    finer or broader search
- `memory_get` to get the full contents of a memory file by name
- `memory_capture` to save "raw" information to the inbox.
- `reference_search` will require a topic + question to show the appropriate doc
  or code. Topics should have embeddings with more than just the name (a small
  explanation of the content) to be queryiable efficiently (for example topic =
  "Rust library Ratatui" + question = "Component implementation" would first
  find the exact topic name through embeddings, then run the question on the
  files of that topic)

---

At regular intervals, the ghosts will take a look at the information in the
inbox and sort it properly. It does not do it directly because information
usually gets refined during the problem solving process.

And when the ghosts re-read the information in the future, they will be
encouraged to improve it: make it shorter, restructure it, validate it, rewrite
unclear parts, remove things that it now has integrated, ...

With models gradually getting better, the notes should improve over time. And
since LLMs are by design outdated (knowledge up to the cutoff date), the
knowledge base will let them continuously improve.

---

There is a local ollama server running for embeddings, and you will use
qwen3-embedding:8b. There should be configuration options for those (embeddings
provider URL, model, ...). You should use the local ollama server for tests.

To have high quality embeddings, use treesitter to parse code AST and embed
functions individually. If you need a specific programming languages list to
support, tell me. For markdown files, embed each section individually.

---

This is an extremely complex feature, requiring thousands of lines of code and
tests.

I want you to:

- Review my specs _in depth_, validating with web search on state of the art on
  those subjects
- Tell me what you would change or implement differently, with the goal being
  high performance, high precision, and low costs (less context, less back and
  forth with tools, less powerful models)
- Once that is done, make an in-depth implementation plan for the feature, if
  necessary in multiple files outlining each step and what needs validation
  after each step.
