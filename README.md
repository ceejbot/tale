# tale

All I wanted was a newline-delimited json log pretty-printer, and they wouldn't give it to me.

## Usage

```
> tale --help
A tool for pretty-printing json logs or any ndjson content that streams to disk.

Tale gives special treatment to the fields that traditionally appear in server logs, such as log level, the text message itself, and the timestamp. It also recognizes log lines that contain the traditional canonical log fields and processes these efficiently, with concise displays.

It's somewhat flexible about what the key fields are named. For example, it takes no side in the "message" vs "msg" wars. No matter what the fiels are, tale will print out every field that shows up in the log line, using the color theme you have set in your terminal.

Usage: tale [OPTIONS] [ARGS]...

Arguments:
  [ARGS]...
          Arguments: [offset] [file] where offset can be -N, +N, or N

Options:
  -t, --timestamps
          Show timestamps, which are hidden by default
  -f, --follow
          Follow the file, continuing to watch for more data to arrive
  -h, --help
          Print help (see a summary with '-h')
  -V, --version
          Print version
```

I have no plans to do any of the other `tail` options. Uh. Other than multi-file tailing.

## Notes

There is a moderately inefficient layout approach that uses [uutils_term_grid](https://github.com/uutils/uutils-term-grid) for the key/value pairs I don't have an opinion about, and some hand-tweaked formatting for the keys I do have an opinion about. If you are using a log format that looks a lot like canonical logs but in json, it's very efficient indeed.

File reading is probably pathological (aka not good) when offsets are very large for very large files. That is, if you say `tale -500000 rilly-long.log` and the file has 500,001 lines, nothing smart will happen. You probably get what you deserve, to be honest. At least memory use won't explode.

I have [ripline](https://lib.rs/crates/ripline) in my back pocket for when I start tailing multiple files at once.

## LICENSE

This code is licensed via [the Parity Public License.](https://paritylicense.com) This license requires people who build on top of this source code to share their work with the community, too. This means if you hack on it for work, you have to make your work repo public somehow. Fair's fair. See the license text for details.
