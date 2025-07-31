# bistre

All I wanted was a newline-delimited json log pretty-printer, and they wouldn't give it to me.

## Usage

At the moment all it does is pretty-print what you pipe into stdin. Sometime quite soon it will tail ndjson log files. And some time very soon after *that* it will tail with an offset from the end of the file. And then it will follow with a `-f` option. I have no plans to do any of the other `tail` options.

## Attribution

There once was a logger named bole.

## LICENSE

This code is licensed via [the Parity Public License.](https://paritylicense.com) This license requires people who build on top of this source code to share their work with the community, too. This means if you hack on it for work, you have to make your work repo public somehow. Fair's fair. See the license text for details.
