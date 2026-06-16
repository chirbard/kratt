# kratt

```sh
cargo run --bin kratt
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
| `output/kratt-graph.toon` | LLM wire format                    |

### graphviz (optional)

```sh
brew install graphviz
dot -Tsvg output/kratt-graph.dot -o output/kratt-graph.svg
```

### cleanup

```sh
rm -rf kratt-graph.sled   # delete sled db
rm -rf output/            # delete generated files
```
