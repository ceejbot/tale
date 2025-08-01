# bistre

All I wanted was a newline-delimited json log pretty-printer, and they wouldn't give it to me.

## Usage

At the moment all it does is pretty-print what you pipe into stdin. Sometime quite soon it will cat a given file name. And some time very soon after *that* it will cat with an offset from the end of the file. And then it will follow with a `-f` option. I have no plans to do any of the other `tail` options.

## Notes

Its behavior is probably pathological (aka not good) when offsets are very large for very large files. That is, if you say `bistre -500000 rilly-long.log` and the file has 500,001 lines, nothing smart will happen. You probably get what you deserve, to be honest. At least memory use won't explode.

## Attribution

There once was a logger named bole. It had a sooty-named pretty-printer. The implementation had nothing in common with this, it being javascript of an earlier era.

## LICENSE

This code is licensed via [the Parity Public License.](https://paritylicense.com) This license requires people who build on top of this source code to share their work with the community, too. This means if you hack on it for work, you have to make your work repo public somehow. Fair's fair. See the license text for details.
