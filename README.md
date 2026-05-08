# tale

A tail-compatible tool for pretty-printing `ndjson` files, especially logs.

All I wanted was a newline-delimited json log pretty-printer, and they wouldn't give it to me. Then this happened. It's missing some polish and a few last small features, but is otherwise complete. If you want to `cat` or `tail` a log file with `ndjson` content, `tale` does a nice job with constant modest memory use. If you want to tail lots of of them, `tale`'s memory use scales predictably. It's more than fast enough.

`tale` recognizes a handful of common log shapes — Stripe-style canonical HTTP logs, log4j/slf4j-style Java records, generic structured-message logs (with field aliases for nginx, k8s, GCP, OpenTelemetry, Docker, etc.), bare timestamped JSON, and [logfmt](https://brandur.org/logfmt). Lines that don't fit any of those are printed as-is.

## Install

```
cargo install tale-ndjson
```

The binary is named `tale`.

## Quick example

```
$ tale fixtures/just_loglines.log
$ cat foo.log | tale
$ tale -n -100 -f /var/log/app.log       # last 100 lines, then follow
$ tale -f *.log                          # follow many files; lines sort by timestamp
```

## Usage

```
> tale --help
A tail-compatible tool for pretty-printing ndjson files, especially logs.

Tale displays the colorfully-formatted contents of FILE, by default stdin, to stdout. It highlights
the fields likely to appear in log lines for servers, such as level or severity, the log message,
timestamps, and so on. It also displays every field that shows up in the log line, using the color
theme you have set in your terminal.

Lines that are invalid json are printed intact, without formatting.

Tale can also follow and display more than one file at a time, with header decoration options like
`tail`'s.

Usage: tale [OPTIONS] [ARGS]...

Arguments:
  [ARGS]...
          (offset) [file ...] where offset can be +N, -N, or N

Options:
  -f, --follow
          Follow the file, continuing to watch for more data to arrive
  -F, --sticky
          Follow the file, also checking to see if has been renamed or has an new inode number. If
          the file does not exist yet, wait and display it from the beginning if and when it is
          created
  -b, --blocks <BLOCKS>
          Accepted for `tail` compatibility but ignored (use -c for byte offsets)
  -c, --bytes <BYTES>
          Start tailing the input offset by ±N bytes; e.g., to skip garbage
  -n, --offset <OFFSET>
          Start tailing the input offset by ±N lines
  -v, --verbose
          When following more than one file, show a header with the file name along with every line
          from that file
  -q, --quiet
          Do not ever show file name headers when following more than one file
  -t, --timestamps
          Show timestamps, which are hidden by default
  -h, --help
          Print help (see a summary with '-h')
  -V, --version
          Print version
```

## Notes

On my MacBook `tale` will pretty-print a million-line file at an approx rate of 387K lines/sec using just under 4MB of memory, steady.

Tale has a set of benchmarks (`cargo bench`) and is optimized for both speed and memory efficiency: a static, file-size-aware chunk sizer, zero-copy JSON deserialization (`Cow<'a, str>` everywhere), and pre-compiled ANSI escape sequences. There are test generator scripts to help benchmark; `tale` behaves less well with those than with real-world log data because real-world data is way more consistent than synthetic data is. Tale is CPU-bound on JSON deserialization, and it is fastest when deserializing into well-understood logging patterns — adding a custom `Printable` variant for a new shape is the path to speed.

To that point: If there is a specific json logging pattern you use that `tale` does not support directly, please give me some samples-- anonymized if you prefer-- and I'll implement deserialization and pretty-printing specifically for that pattern. It's more than fast enough, however, and it's just memory use I watch carefully.

There is fairly stupid single-pass layout approach to print in columns the key/value pairs I don't have an opinion. It isn't very pretty because it is single-pass. There is hand-tweaked formatting for the keys I do have an opinion about.

## LICENSE

This code is licensed via [the Parity Public License.](https://paritylicense.com) This license requires people who build on top of this source code to share their work with the community, too. This means if you hack on it for work, you have to make your work repo public somehow. Fair's fair. See the license text for details.
