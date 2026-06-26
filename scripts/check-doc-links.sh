#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

ruby <<'RUBY'
require "pathname"
require "uri"

root = Pathname.new(Dir.pwd)
files = ["README.md", "AGENTS.md"] + Dir.glob("docs/**/*.md")
files.select! { |path| File.file?(path) }

missing = []

files.each do |file|
  text = File.read(file)
  text.scan(/!?\[[^\]]*\]\(([^)]+)\)/) do |match|
    dest = match.first.strip
    dest = dest[1...-1] if dest.start_with?("<") && dest.end_with?(">")
    dest = dest.split(/\s+/, 2).first
    next if dest.nil? || dest.empty?
    next if dest.start_with?("#")
    next if dest.start_with?("//")
    next if dest.match?(/\A[a-z][a-z0-9+.-]*:/i)

    path = dest.split("#", 2).first
    next if path.nil? || path.empty?

    begin
      path = URI::DEFAULT_PARSER.unescape(path)
    rescue ArgumentError
      # Keep the original text if the link contains a literal percent.
    end

    target = (root + File.dirname(file) + path).cleanpath
    missing << "#{file}: #{dest}" unless target.exist?
  end
end

if missing.empty?
  puts "All checked local markdown links resolve."
else
  warn "Broken local markdown links:"
  missing.each { |line| warn "  #{line}" }
  exit 1
end
RUBY
