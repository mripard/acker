# Tool to send review automatically on mailing lists

Linux, and several other related projects, use tags sent by e-mails to provide
a notification to the author that their mail has been reviewed or tested.

`acker` aims at providing an automated way to send those reviews.

# Configuration

`acker` will reuse the author name, email and SMTP setup of their git configuration.

# Example

```
$ cat mail | acker -r
```
