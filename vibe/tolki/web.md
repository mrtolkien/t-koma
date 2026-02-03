# Web tools

We have a nice set of tools at the moment, but we lack the most important ones:
web access.

There needs to be at least two types of tools:

- A direct web_fetch tool: it should only return the textual content of the
  page, possible converted to Markdown
- A web_search tool

Here are the Claude versions:

````
# Web fetch tool

---

The web fetch tool allows Claude to retrieve full content from specified web pages and PDF documents.

<Note>
The web fetch tool is currently in beta. To enable it, use the beta header `web-fetch-2025-09-10` in your API requests.

Please use [this form](https://forms.gle/NhWcgmkcvPCMmPE86) to provide feedback on the quality of the model responses, the API itself, or the quality of the documentation.
</Note>

<Warning>
Enabling the web fetch tool in environments where Claude processes untrusted input alongside sensitive data poses data exfiltration risks. We recommend only using this tool in trusted environments or when handling non-sensitive data.

To minimize exfiltration risks, Claude is not allowed to dynamically construct URLs. Claude can only fetch URLs that have been explicitly provided by the user or that come from previous web search or web fetch results. However, there is still residual risk that should be carefully considered when using this tool.

If data exfiltration is a concern, consider:
- Disabling the web fetch tool entirely
- Using the `max_uses` parameter to limit the number of requests
- Using the `allowed_domains` parameter to restrict to known safe domains
</Warning>

## Supported models

Web fetch is available on:

- Claude Sonnet 4.5 (`claude-sonnet-4-5-20250929`)
- Claude Sonnet 4 (`claude-sonnet-4-20250514`)
- Claude Sonnet 3.7 ([deprecated](/docs/en/about-claude/model-deprecations)) (`claude-3-7-sonnet-20250219`)
- Claude Haiku 4.5 (`claude-haiku-4-5-20251001`)
- Claude Haiku 3.5 ([deprecated](/docs/en/about-claude/model-deprecations)) (`claude-3-5-haiku-latest`)
- Claude Opus 4.5 (`claude-opus-4-5-20251101`)
- Claude Opus 4.1 (`claude-opus-4-1-20250805`)
- Claude Opus 4 (`claude-opus-4-20250514`)

## How web fetch works

When you add the web fetch tool to your API request:

1. Claude decides when to fetch content based on the prompt and available URLs.
2. The API retrieves the full text content from the specified URL.
3. For PDFs, automatic text extraction is performed.
4. Claude analyzes the fetched content and provides a response with optional citations.

<Note>
The web fetch tool currently does not support web sites dynamically rendered via Javascript.
</Note>

## How to use web fetch

Provide the web fetch tool in your API request:

<CodeGroup>
```bash Shell
curl https://api.anthropic.com/v1/messages \
    --header "x-api-key: $ANTHROPIC_API_KEY" \
    --header "anthropic-version: 2023-06-01" \
    --header "anthropic-beta: web-fetch-2025-09-10" \
    --header "content-type: application/json" \
    --data '{
        "model": "claude-sonnet-4-5",
        "max_tokens": 1024,
        "messages": [
            {
                "role": "user",
                "content": "Please analyze the content at https://example.com/article"
            }
        ],
        "tools": [{
            "type": "web_fetch_20250910",
            "name": "web_fetch",
            "max_uses": 5
        }]
    }'
````

```python Python
import anthropic

client = anthropic.Anthropic()

response = client.messages.create(
    model="claude-sonnet-4-5",
    max_tokens=1024,
    messages=[
        {
            "role": "user",
            "content": "Please analyze the content at https://example.com/article"
        }
    ],
    tools=[{
        "type": "web_fetch_20250910",
        "name": "web_fetch",
        "max_uses": 5
    }],
    extra_headers={
        "anthropic-beta": "web-fetch-2025-09-10"
    }
)
print(response)
```

```typescript TypeScript
import { Anthropic } from "@anthropic-ai/sdk";

const anthropic = new Anthropic();

async function main() {
  const response = await anthropic.messages.create({
    model: "claude-sonnet-4-5",
    max_tokens: 1024,
    messages: [
      {
        role: "user",
        content: "Please analyze the content at https://example.com/article",
      },
    ],
    tools: [
      {
        type: "web_fetch_20250910",
        name: "web_fetch",
        max_uses: 5,
      },
    ],
    headers: {
      "anthropic-beta": "web-fetch-2025-09-10",
    },
  });

  console.log(response);
}

main().catch(console.error);
```

</CodeGroup>

### Tool definition

The web fetch tool supports the following parameters:

```json JSON
{
  "type": "web_fetch_20250910",
  "name": "web_fetch",

  // Optional: Limit the number of fetches per request
  "max_uses": 10,

  // Optional: Only fetch from these domains
  "allowed_domains": ["example.com", "docs.example.com"],

  // Optional: Never fetch from these domains
  "blocked_domains": ["private.example.com"],

  // Optional: Enable citations for fetched content
  "citations": {
    "enabled": true
  },

  // Optional: Maximum content length in tokens
  "max_content_tokens": 100000
}
```

#### Max uses

The `max_uses` parameter limits the number of web fetches performed. If Claude
attempts more fetches than allowed, the `web_fetch_tool_result` will be an error
with the `max_uses_exceeded` error code. There is currently no default limit.

#### Domain filtering

When using domain filters:

- Domains should not include the HTTP/HTTPS scheme (use `example.com` instead of
  `https://example.com`)
- Subdomains are automatically included (`example.com` covers
  `docs.example.com`)
- Subpaths are supported (`example.com/blog`)
- You can use either `allowed_domains` or `blocked_domains`, but not both in the
  same request.

<Warning>
Be aware that Unicode characters in domain names can create security vulnerabilities through homograph attacks, where visually similar characters from different scripts can bypass domain filters. For example, `аmazon.com` (using Cyrillic 'а') may appear identical to `amazon.com` but represents a different domain.

When configuring domain allow/block lists:

- Use ASCII-only domain names when possible
- Consider that URL parsers may handle Unicode normalization differently
- Test your domain filters with potential homograph variations
- Regularly audit your domain configurations for suspicious Unicode characters
  </Warning>

#### Content limits

The `max_content_tokens` parameter limits the amount of content that will be
included in the context. If the fetched content exceeds this limit, it will be
truncated. This helps control token usage when fetching large documents.

<Note>
The `max_content_tokens` parameter limit is approximate. The actual number of input tokens used can vary by a small amount.
</Note>

#### Citations

Unlike web search where citations are always enabled, citations are optional for
web fetch. Set `"citations": {"enabled": true}` to enable Claude to cite
specific passages from fetched documents.

<Note>
When displaying API outputs directly to end users, citations must be included to the original source. If you are making modifications to API outputs, including by reprocessing and/or combining them with your own material before displaying them to end users, display citations as appropriate based on consultation with your legal team.
</Note>

### Response

Here's an example response structure:

```json
{
  "role": "assistant",
  "content": [
    // 1. Claude's decision to fetch
    {
      "type": "text",
      "text": "I'll fetch the content from the article to analyze it."
    },
    // 2. The fetch request
    {
      "type": "server_tool_use",
      "id": "srvtoolu_01234567890abcdef",
      "name": "web_fetch",
      "input": {
        "url": "https://example.com/article"
      }
    },
    // 3. Fetch results
    {
      "type": "web_fetch_tool_result",
      "tool_use_id": "srvtoolu_01234567890abcdef",
      "content": {
        "type": "web_fetch_result",
        "url": "https://example.com/article",
        "content": {
          "type": "document",
          "source": {
            "type": "text",
            "media_type": "text/plain",
            "data": "Full text content of the article..."
          },
          "title": "Article Title",
          "citations": { "enabled": true }
        },
        "retrieved_at": "2025-08-25T10:30:00Z"
      }
    },
    // 4. Claude's analysis with citations (if enabled)
    {
      "text": "Based on the article, ",
      "type": "text"
    },
    {
      "text": "the main argument presented is that artificial intelligence will transform healthcare",
      "type": "text",
      "citations": [
        {
          "type": "char_location",
          "document_index": 0,
          "document_title": "Article Title",
          "start_char_index": 1234,
          "end_char_index": 1456,
          "cited_text": "Artificial intelligence is poised to revolutionize healthcare delivery..."
        }
      ]
    }
  ],
  "id": "msg_a930390d3a",
  "usage": {
    "input_tokens": 25039,
    "output_tokens": 931,
    "server_tool_use": {
      "web_fetch_requests": 1
    }
  },
  "stop_reason": "end_turn"
}
```

#### Fetch results

Fetch results include:

- `url`: The URL that was fetched
- `content`: A document block containing the fetched content
- `retrieved_at`: Timestamp when the content was retrieved

<Note>
The web fetch tool caches results to improve performance and reduce redundant requests. This means the content returned may not always be the latest version available at the URL. The cache behavior is managed automatically and may change over time to optimize for different content types and usage patterns.
</Note>

For PDF documents, the content will be returned as base64-encoded data:

```json
{
  "type": "web_fetch_tool_result",
  "tool_use_id": "srvtoolu_02",
  "content": {
    "type": "web_fetch_result",
    "url": "https://example.com/paper.pdf",
    "content": {
      "type": "document",
      "source": {
        "type": "base64",
        "media_type": "application/pdf",
        "data": "JVBERi0xLjQKJcOkw7zDtsOfCjIgMCBvYmo..."
      },
      "citations": { "enabled": true }
    },
    "retrieved_at": "2025-08-25T10:30:02Z"
  }
}
```

#### Errors

When the web fetch tool encounters an error, the Claude API returns a 200
(success) response with the error represented in the response body:

```json
{
  "type": "web_fetch_tool_result",
  "tool_use_id": "srvtoolu_a93jad",
  "content": {
    "type": "web_fetch_tool_error",
    "error_code": "url_not_accessible"
  }
}
```

These are the possible error codes:

- `invalid_input`: Invalid URL format
- `url_too_long`: URL exceeds maximum length (250 characters)
- `url_not_allowed`: URL blocked by domain filtering rules and model
  restrictions
- `url_not_accessible`: Failed to fetch content (HTTP error)
- `too_many_requests`: Rate limit exceeded
- `unsupported_content_type`: Content type not supported (only text and PDF)
- `max_uses_exceeded`: Maximum web fetch tool uses exceeded
- `unavailable`: An internal error occurred

## URL validation

For security reasons, the web fetch tool can only fetch URLs that have
previously appeared in the conversation context. This includes:

- URLs in user messages
- URLs in client-side tool results
- URLs from previous web search or web fetch results

The tool cannot fetch arbitrary URLs that Claude generates or URLs from
container-based server tools (Code Execution, Bash, etc.).

## Combined search and fetch

Web fetch works seamlessly with web search for comprehensive information
gathering:

```python
import anthropic

client = anthropic.Anthropic()

response = client.messages.create(
    model="claude-sonnet-4-5",
    max_tokens=4096,
    messages=[
        {
            "role": "user",
            "content": "Find recent articles about quantum computing and analyze the most relevant one in detail"
        }
    ],
    tools=[
        {
            "type": "web_search_20250305",
            "name": "web_search",
            "max_uses": 3
        },
        {
            "type": "web_fetch_20250910",
            "name": "web_fetch",
            "max_uses": 5,
            "citations": {"enabled": True}
        }
    ],
    extra_headers={
        "anthropic-beta": "web-fetch-2025-09-10"
    }
)
```

In this workflow, Claude will:

1. Use web search to find relevant articles
2. Select the most promising results
3. Use web fetch to retrieve full content
4. Provide detailed analysis with citations

## Prompt caching

Web fetch works with
[prompt caching](/docs/en/build-with-claude/prompt-caching). To enable prompt
caching, add `cache_control` breakpoints in your request. Cached fetch results
can be reused across conversation turns.

```python
import anthropic

client = anthropic.Anthropic()

# First request with web fetch
messages = [
    {
        "role": "user",
        "content": "Analyze this research paper: https://arxiv.org/abs/2024.12345"
    }
]

response1 = client.messages.create(
    model="claude-sonnet-4-5",
    max_tokens=1024,
    messages=messages,
    tools=[{
        "type": "web_fetch_20250910",
        "name": "web_fetch"
    }],
    extra_headers={
        "anthropic-beta": "web-fetch-2025-09-10"
    }
)

# Add Claude's response to conversation
messages.append({
    "role": "assistant",
    "content": response1.content
})

# Second request with cache breakpoint
messages.append({
    "role": "user",
    "content": "What methodology does the paper use?",
    "cache_control": {"type": "ephemeral"}
})

response2 = client.messages.create(
    model="claude-sonnet-4-5",
    max_tokens=1024,
    messages=messages,
    tools=[{
        "type": "web_fetch_20250910",
        "name": "web_fetch"
    }],
    extra_headers={
        "anthropic-beta": "web-fetch-2025-09-10"
    }
)

# The second response benefits from cached fetch results
print(f"Cache read tokens: {response2.usage.get('cache_read_input_tokens', 0)}")
```

## Streaming

With streaming enabled, fetch events are part of the stream with a pause during
content retrieval:

```javascript
event: message_start
data: {"type": "message_start", "message": {"id": "msg_abc123", "type": "message"}}

event: content_block_start
data: {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}

// Claude's decision to fetch

event: content_block_start
data: {"type": "content_block_start", "index": 1, "content_block": {"type": "server_tool_use", "id": "srvtoolu_xyz789", "name": "web_fetch"}}

// Fetch URL streamed
event: content_block_delta
data: {"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "{\"url\":\"https://example.com/article\"}"}}

// Pause while fetch executes

// Fetch results streamed
event: content_block_start
data: {"type": "content_block_start", "index": 2, "content_block": {"type": "web_fetch_tool_result", "tool_use_id": "srvtoolu_xyz789", "content": {"type": "web_fetch_result", "url": "https://example.com/article", "content": {"type": "document", "source": {"type": "text", "media_type": "text/plain", "data": "Article content..."}}}}}

// Claude's response continues...
```

## Batch requests

You can include the web fetch tool in the
[Messages Batches API](/docs/en/build-with-claude/batch-processing). Web fetch
tool calls through the Messages Batches API are priced the same as those in
regular Messages API requests.

## Usage and pricing

Web fetch usage has **no additional charges** beyond standard token costs:

```json
"usage": {
  "input_tokens": 25039,
  "output_tokens": 931,
  "cache_read_input_tokens": 0,
  "cache_creation_input_tokens": 0,
  "server_tool_use": {
    "web_fetch_requests": 1
  }
}
```

The web fetch tool is available on the Claude API at **no additional cost**. You
only pay standard token costs for the fetched content that becomes part of your
conversation context.

To protect against inadvertently fetching large content that would consume
excessive tokens, use the `max_content_tokens` parameter to set appropriate
limits based on your use case and budget considerations.

Example token usage for typical content:

- Average web page (10KB): ~2,500 tokens
- Large documentation page (100KB): ~25,000 tokens
- Research paper PDF (500KB): ~125,000 tokens

```

```

# Web search tool

---

The web search tool gives Claude direct access to real-time web content,
allowing it to answer questions with up-to-date information beyond its knowledge
cutoff. Claude automatically cites sources from search results as part of its
answer.

<Note>
Please reach out through our [feedback form](https://forms.gle/sWjBtsrNEY2oKGuE8) to share your experience with the web search tool.
</Note>

## Supported models

Web search is available on:

- Claude Sonnet 4.5 (`claude-sonnet-4-5-20250929`)
- Claude Sonnet 4 (`claude-sonnet-4-20250514`)
- Claude Sonnet 3.7 ([deprecated](/docs/en/about-claude/model-deprecations))
  (`claude-3-7-sonnet-20250219`)
- Claude Haiku 4.5 (`claude-haiku-4-5-20251001`)
- Claude Haiku 3.5 ([deprecated](/docs/en/about-claude/model-deprecations))
  (`claude-3-5-haiku-latest`)
- Claude Opus 4.5 (`claude-opus-4-5-20251101`)
- Claude Opus 4.1 (`claude-opus-4-1-20250805`)
- Claude Opus 4 (`claude-opus-4-20250514`)

## How web search works

When you add the web search tool to your API request:

1. Claude decides when to search based on the prompt.
2. The API executes the searches and provides Claude with the results. This
   process may repeat multiple times throughout a single request.
3. At the end of its turn, Claude provides a final response with cited sources.

## How to use web search

<Note>
Your organization's administrator must enable web search in [Console](/settings/privacy).
</Note>

Provide the web search tool in your API request:

<CodeGroup>
```bash Shell
curl https://api.anthropic.com/v1/messages \
    --header "x-api-key: $ANTHROPIC_API_KEY" \
    --header "anthropic-version: 2023-06-01" \
    --header "content-type: application/json" \
    --data '{
        "model": "claude-sonnet-4-5",
        "max_tokens": 1024,
        "messages": [
            {
                "role": "user",
                "content": "What is the weather in NYC?"
            }
        ],
        "tools": [{
            "type": "web_search_20250305",
            "name": "web_search",
            "max_uses": 5
        }]
    }'
```

```python Python
import anthropic

client = anthropic.Anthropic()

response = client.messages.create(
    model="claude-sonnet-4-5",
    max_tokens=1024,
    messages=[
        {
            "role": "user",
            "content": "What's the weather in NYC?"
        }
    ],
    tools=[{
        "type": "web_search_20250305",
        "name": "web_search",
        "max_uses": 5
    }]
)
print(response)
```

```typescript TypeScript
import { Anthropic } from "@anthropic-ai/sdk";

const anthropic = new Anthropic();

async function main() {
  const response = await anthropic.messages.create({
    model: "claude-sonnet-4-5",
    max_tokens: 1024,
    messages: [
      {
        role: "user",
        content: "What's the weather in NYC?",
      },
    ],
    tools: [
      {
        type: "web_search_20250305",
        name: "web_search",
        max_uses: 5,
      },
    ],
  });

  console.log(response);
}

main().catch(console.error);
```

</CodeGroup>

### Tool definition

The web search tool supports the following parameters:

```json JSON
{
  "type": "web_search_20250305",
  "name": "web_search",

  // Optional: Limit the number of searches per request
  "max_uses": 5,

  // Optional: Only include results from these domains
  "allowed_domains": ["example.com", "trusteddomain.org"],

  // Optional: Never include results from these domains
  "blocked_domains": ["untrustedsource.com"],

  // Optional: Localize search results
  "user_location": {
    "type": "approximate",
    "city": "San Francisco",
    "region": "California",
    "country": "US",
    "timezone": "America/Los_Angeles"
  }
}
```

#### Max uses

The `max_uses` parameter limits the number of searches performed. If Claude
attempts more searches than allowed, the `web_search_tool_result` will be an
error with the `max_uses_exceeded` error code.

#### Domain filtering

When using domain filters:

- Domains should not include the HTTP/HTTPS scheme (use `example.com` instead of
  `https://example.com`)
- Subdomains are automatically included (`example.com` covers
  `docs.example.com`)
- Specific subdomains restrict results to only that subdomain
  (`docs.example.com` returns only results from that subdomain, not from
  `example.com` or `api.example.com`)
- Subpaths are supported and match anything after the path (`example.com/blog`
  matches `example.com/blog/post-1`)
- You can use either `allowed_domains` or `blocked_domains`, but not both in the
  same request.

**Wildcard support:**

- Only one wildcard (`*`) is allowed per domain entry, and it must appear after
  the domain part (in the path)
- Valid: `example.com/*`, `example.com/*/articles`
- Invalid: `*.example.com`, `ex*.com`, `example.com/*/news/*`

Invalid domain formats will return an `invalid_tool_input` tool error.

<Note>
Request-level domain restrictions must be compatible with organization-level domain restrictions configured in the Console. Request-level domains can only further restrict domains, not override or expand beyond the organization-level list. If your request includes domains that conflict with organization settings, the API will return a validation error.
</Note>

#### Localization

The `user_location` parameter allows you to localize search results based on a
user's location.

- `type`: The type of location (must be `approximate`)
- `city`: The city name
- `region`: The region or state
- `country`: The country
- `timezone`: The
  [IANA timezone ID](https://en.wikipedia.org/wiki/List_of_tz_database_time_zones).

### Response

Here's an example response structure:

```json
{
  "role": "assistant",
  "content": [
    // 1. Claude's decision to search
    {
      "type": "text",
      "text": "I'll search for when Claude Shannon was born."
    },
    // 2. The search query used
    {
      "type": "server_tool_use",
      "id": "srvtoolu_01WYG3ziw53XMcoyKL4XcZmE",
      "name": "web_search",
      "input": {
        "query": "claude shannon birth date"
      }
    },
    // 3. Search results
    {
      "type": "web_search_tool_result",
      "tool_use_id": "srvtoolu_01WYG3ziw53XMcoyKL4XcZmE",
      "content": [
        {
          "type": "web_search_result",
          "url": "https://en.wikipedia.org/wiki/Claude_Shannon",
          "title": "Claude Shannon - Wikipedia",
          "encrypted_content": "EqgfCioIARgBIiQ3YTAwMjY1Mi1mZjM5LTQ1NGUtODgxNC1kNjNjNTk1ZWI3Y...",
          "page_age": "April 30, 2025"
        }
      ]
    },
    {
      "text": "Based on the search results, ",
      "type": "text"
    },
    // 4. Claude's response with citations
    {
      "text": "Claude Shannon was born on April 30, 1916, in Petoskey, Michigan",
      "type": "text",
      "citations": [
        {
          "type": "web_search_result_location",
          "url": "https://en.wikipedia.org/wiki/Claude_Shannon",
          "title": "Claude Shannon - Wikipedia",
          "encrypted_index": "Eo8BCioIAhgBIiQyYjQ0OWJmZi1lNm..",
          "cited_text": "Claude Elwood Shannon (April 30, 1916 – February 24, 2001) was an American mathematician, electrical engineer, computer scientist, cryptographer and i..."
        }
      ]
    }
  ],
  "id": "msg_a930390d3a",
  "usage": {
    "input_tokens": 6039,
    "output_tokens": 931,
    "server_tool_use": {
      "web_search_requests": 1
    }
  },
  "stop_reason": "end_turn"
}
```

#### Search results

Search results include:

- `url`: The URL of the source page
- `title`: The title of the source page
- `page_age`: When the site was last updated
- `encrypted_content`: Encrypted content that must be passed back in multi-turn
  conversations for citations

#### Citations

Citations are always enabled for web search, and each
`web_search_result_location` includes:

- `url`: The URL of the cited source
- `title`: The title of the cited source
- `encrypted_index`: A reference that must be passed back for multi-turn
  conversations.
- `cited_text`: Up to 150 characters of the cited content

The web search citation fields `cited_text`, `title`, and `url` do not count
towards input or output token usage.

<Note>
  When displaying API outputs directly to end users, citations must be included to the original source. If you are making modifications to API outputs, including by reprocessing and/or combining them with your own material before displaying them to end users, display citations as appropriate based on consultation with your legal team.
</Note>

#### Errors

When the web search tool encounters an error (such as hitting rate limits), the
Claude API still returns a 200 (success) response. The error is represented
within the response body using the following structure:

```json
{
  "type": "web_search_tool_result",
  "tool_use_id": "servertoolu_a93jad",
  "content": {
    "type": "web_search_tool_result_error",
    "error_code": "max_uses_exceeded"
  }
}
```

These are the possible error codes:

- `too_many_requests`: Rate limit exceeded
- `invalid_input`: Invalid search query parameter
- `max_uses_exceeded`: Maximum web search tool uses exceeded
- `query_too_long`: Query exceeds maximum length
- `unavailable`: An internal error occurred

#### `pause_turn` stop reason

The response may include a `pause_turn` stop reason, which indicates that the
API paused a long-running turn. You may provide the response back as-is in a
subsequent request to let Claude continue its turn, or modify the content if you
wish to interrupt the conversation.

## Prompt caching

Web search works with
[prompt caching](/docs/en/build-with-claude/prompt-caching). To enable prompt
caching, add at least one `cache_control` breakpoint in your request. The system
will automatically cache up until the last `web_search_tool_result` block when
executing the tool.

For multi-turn conversations, set a `cache_control` breakpoint on or after the
last `web_search_tool_result` block to reuse cached content.

For example, to use prompt caching with web search for a multi-turn
conversation:

<CodeGroup>
```python
import anthropic

client = anthropic.Anthropic()

# First request with web search and cache breakpoint

messages = [ { "role": "user", "content": "What's the current weather in San
Francisco today?" } ]

response1 = client.messages.create( model="claude-sonnet-4-5", max_tokens=1024,
messages=messages, tools=[{ "type": "web_search_20250305", "name": "web_search",
"user_location": { "type": "approximate", "city": "San Francisco", "region":
"California", "country": "US", "timezone": "America/Los_Angeles" } }] )

# Add Claude's response to the conversation

messages.append({ "role": "assistant", "content": response1.content })

# Second request with cache breakpoint after the search results

messages.append({ "role": "user", "content": "Should I expect rain later this
week?", "cache_control": {"type": "ephemeral"} # Cache up to this point })

response2 = client.messages.create( model="claude-sonnet-4-5", max_tokens=1024,
messages=messages, tools=[{ "type": "web_search_20250305", "name": "web_search",
"user_location": { "type": "approximate", "city": "San Francisco", "region":
"California", "country": "US", "timezone": "America/Los_Angeles" } }] )

# The second response will benefit from cached search results

# while still being able to perform new searches if needed

print(f"Cache read tokens: {response2.usage.get('cache_read_input_tokens', 0)}")

````

</CodeGroup>

## Streaming

With streaming enabled, you'll receive search events as part of the stream. There will be a pause while the search executes:

```javascript
event: message_start
data: {"type": "message_start", "message": {"id": "msg_abc123", "type": "message"}}

event: content_block_start
data: {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}

// Claude's decision to search

event: content_block_start
data: {"type": "content_block_start", "index": 1, "content_block": {"type": "server_tool_use", "id": "srvtoolu_xyz789", "name": "web_search"}}

// Search query streamed
event: content_block_delta
data: {"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "{\"query\":\"latest quantum computing breakthroughs 2025\"}"}}

// Pause while search executes

// Search results streamed
event: content_block_start
data: {"type": "content_block_start", "index": 2, "content_block": {"type": "web_search_tool_result", "tool_use_id": "srvtoolu_xyz789", "content": [{"type": "web_search_result", "title": "Quantum Computing Breakthroughs in 2025", "url": "https://example.com"}]}}

// Claude's response with citations (omitted in this example)
````

## Batch requests

You can include the web search tool in the
[Messages Batches API](/docs/en/build-with-claude/batch-processing). Web search
tool calls through the Messages Batches API are priced the same as those in
regular Messages API requests.

## Usage and pricing

Web search usage is charged in addition to token usage:

```json
"usage": {
  "input_tokens": 105,
  "output_tokens": 6039,
  "cache_read_input_tokens": 7123,
  "cache_creation_input_tokens": 7345,
  "server_tool_use": {
    "web_search_requests": 1
  }
}
```

Web search is available on the Claude API for **$10 per 1,000 searches**, plus
standard token costs for search-generated content. Web search results retrieved
throughout a conversation are counted as input tokens, in search iterations
executed during a single turn and in subsequent conversation turns.

Each web search counts as one use, regardless of the number of results returned.
If an error occurs during web search, the web search will not be billed.

```

```

I want you to:

- Research web fetch and search options and see how they are implemented in
  other places (openclaw, brave api, z.ai api, ...)
- Implement them, possibly with the option to use an external API for either and
  set a list of possible web search providers. You can add configuration
  options, for example letting the user configure multiple options and then
  using an ordered list for usage. The agent should only see a single, simple
  tool.

Make sure the code is clean and well organized because it will be complex and
long.
