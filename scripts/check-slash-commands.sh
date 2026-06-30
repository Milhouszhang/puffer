#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

ruby <<'RUBY'
registry_path = "crates/puffer-core/command/registry.rs"
doc_path = "docs/reference/slash-commands.md"

registry = File.read(registry_path)
doc = File.read(doc_path)

commands = registry.scan(/cmd(?:_hidden)?\(\s*"([^"]+)"/m).flatten.uniq.sort
missing = commands.reject { |name| doc.include?("/#{name}") }

if missing.empty?
  puts "Slash-command reference covers #{commands.length} built-in commands."
else
  warn "Slash-command reference is missing:"
  missing.each { |name| warn "  /#{name}" }
  exit 1
end
RUBY
