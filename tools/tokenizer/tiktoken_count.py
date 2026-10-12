"""Count tokens with a real BPE encoder, for `fgx token-report --bpe-command`.

Reads the text to measure from stdin, exactly as Forgentex sends it, and prints
one integer. The encoding name is the only argument (default `o200k_base`).

    fgx token-report examples/hello.fgx \
        --bpe-command python --bpe-arg tools/tokenizer/tiktoken_count.py \
        --bpe-arg o200k_base --bpe-name tiktoken:o200k_base

Requires `pip install tiktoken`. The count covers the text alone: no chat
framing, system prompt, tool schema, or reasoning — so it compares surfaces with
each other, not the cost of a request.
"""

import sys

import tiktoken


def main() -> None:
    encoding = tiktoken.get_encoding(sys.argv[1] if len(sys.argv) > 1 else "o200k_base")
    text = sys.stdin.buffer.read().decode("utf-8")
    # `disallowed_special=()` so source that happens to contain a special-token
    # spelling is counted as ordinary text instead of raising.
    print(len(encoding.encode(text, disallowed_special=())))


if __name__ == "__main__":
    main()
