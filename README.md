# kratt

## env vars

| var                  | default                      | description               |
| -------------------- | ---------------------------- | ------------------------- |
| `KRATT_BACKEND`      | `ollama`                     | `ollama` or `openrouter`  |
| `OLLAMA_MODEL`       | `gemma4:e4b`                 | model name for Ollama     |
| `OPENROUTER_API_KEY` | required                     | API key for OpenRouter    |
| `OPENROUTER_MODEL`   | `deepseek/deepseek-v4-flash` | model name for OpenRouter |

```sh
# pass prompt as argument
cargo run --bin kratt "your message here"

# or start and type when prompted
cargo run --bin kratt

# run with openrouter
KRATT_BACKEND=openrouter cargo run --bin kratt "your message here"

# run with a different ollama model
OLLAMA_MODEL=qwen2.5:14b cargo run --bin kratt "your message here"
```

## viz

Converts a TOON graph file to GEXF, DOT, and SVG.

```sh
# convert live memory graph (default)
cargo run --bin viz

# convert a specific toon file
cargo run --bin viz path/to/file.toon

# open in gephi
open output/kratt-graph.gexf

# open rendered svg
open output/kratt-graph.svg
```

## graph_poc

Reads `kratt-memory/` → sled → petgraph → DOT / GEXF / TOON / SVG.

```sh
# run
cargo run --bin graph_poc

# open graph
open output/kratt-graph.svg

# open in gephi
open output/kratt-graph.gexf
```

### output files

| file                      | use                                |
| ------------------------- | ---------------------------------- |
| `output/kratt-graph.dot`  | graphviz source                    |
| `output/kratt-graph.svg`  | rendered graph (requires graphviz) |
| `output/kratt-graph.gexf` | Gephi import                       |
| `output/kratt-graph.toon` | LLM wire format (graph_poc)        |

### graphviz (optional)

```sh
brew install graphviz
dot -Tsvg output/kratt-graph.dot -o output/kratt-graph.svg
```

## memory (kratt binary)

Graph is stored in `kratt-data/` as its own git repo (gitignored by the main repo).
Each agent step that modifies the graph commits automatically.

```sh
# inspect memory history
git -C kratt-data log --oneline

# revert to empty state
git -C kratt-data checkout init -- kratt-graph.toon

# reset memory entirely
rm -rf kratt-data/
```

### cleanup

```sh
rm -rf kratt-graph.sled   # delete sled db
rm -rf output/            # delete graph_poc output
rm -rf kratt-data/        # delete memory graph and its git history
```
