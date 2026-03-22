# H Language Support (VS Code Extension)

This extension adds language support for H (`.hl`) files in VS Code.

## Features

- Recognizes `.hl` files as H language
- Syntax highlighting for:
  - sections, functions, keywords, mnemonics
  - registers (`r1`, `r2`, ...)
  - Java-style typed declarations (`int`, `string`, `bool`)
  - operators, numbers, strings, comments
  - YAML-like key/value fields in `print:` blocks
- Bracket/quote auto-closing
- Line comments with `//`
- Snippets for program skeleton, print blocks, and if/else

## Usage

1. Open this extension folder in VS Code:
   - `vscode-h-language`
2. Press `F5` to launch an Extension Development Host.
3. Open any `.hl` file and verify language mode is `H`.

## Publish (optional)

If you want to publish to Marketplace, update `publisher` in `package.json` and follow `vsce` publish flow.
