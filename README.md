# Juno Proxy Server

## Usage

```console
$ juno --help
Juno Proxy Server

Usage: juno --listen-stream <ADDRESS> --provider <NAME>

Options:
  -l, --listen-stream <ADDRESS>  Specifies an address to listen on for a stream
  -b, --bind-to <ADDRESS>        Specifies the source address of outbound connections
  -p, --provider <NAME>          Specifies the name of the service provider
  -h, --help                     Print help information
  -V, --version                  Print version information
```

### launchd support (macOS only)

Create a property list file (e.g. `~/Library/LaunchAgents/com.github.dacci.juno.plist`) with appropriate parameters.
See `launchd.plist(5)` for details.

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>com.github.dacci.juno</string>
	<key>ProgramArguments</key>
	<array>
		<string>/path/to/juno</string>
		<string>--provider</string>
		<string>http</string>
		<string>--launchd</string>
		<string>Listeners</string>
	</array>
	<key>Sockets</key>
	<dict>
		<key>Listeners</key>
		<dict>
			<key>SockNodeName</key>
			<string>127.0.0.1</string>
			<key>SockServiceName</key>
			<integer>8080</integer>
		</dict>
	</dict>
</dict>
</plist>
```

Run `launchctl` to load the property list file:

```console
$ launchctl load ~/Library/LaunchAgents/com.github.dacci.juno.plist
```

### systemd support (Linux only)

Create unit files with appropriate parameters and run `systemctl` to start daemon.

```console
$ systemctl --user daemon-reload
$ systemctl --user start juno.socket
```

#### Socket unit configuration example (~/.config/systemd/user/juno.socket)

See [systemd.socket(5)](https://www.freedesktop.org/software/systemd/man/systemd.socket.html) for details.

```
[Socket]
ListenStream=127.0.0.1:1080
```

#### Service unit configuration example (~/.config/systemd/user/juno.service)

See [systemd.service(5)](https://www.freedesktop.org/software/systemd/man/systemd.service.html) for details.

```
[Service]
ExecStart=/path/to/juno --provider socks --systemd
```
