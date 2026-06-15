# graph_poc

Reads `kratt-memory/` markdown files, stores nodes and edges in sled, builds a petgraph graph, and exports DOT for graphviz.

```sh
# run
cargo run --bin graph_poc

# render SVG (requires graphviz)
brew install graphviz
dot -Tsvg output/kratt-graph.dot -o output/kratt-graph.svg

# other output formats
dot -Tpng output/kratt-graph.dot -o output/kratt-graph.png
dot -Tpdf output/kratt-graph.dot -o output/kratt-graph.pdf

# open result
open output/kratt-graph.svg

# inspect sled db
cargo run --bin graph_poc  # repopulates on every run

# delete sled db
rm -rf kratt-graph.sled
```
