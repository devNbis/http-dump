# ![Alt logo](./assets/logo.svg) HTTP(S) Dump tool

This is a simple single file request response dump tool for http(s) traffic into log or live into a web side without persistence.

It is written in RUST and can be used wherever a target compilation can run.

## Local execution

Build the project with

```sh
cargo install --path .
```
Copy the executable or execute from the building folder 
```sh
./target/release/http-dump
```

To get the possible options or environment variables call
```sh
$ http-dump --help    
Programm for dump http(s) traffic into system log or live http view

Usage: http-dump [OPTIONS]

Options:
  -b, --bind <IP:Port>   host address include port [env: HTTP_DUMP_BIND=] [default: 0.0.0.0:8089]
  -e, --error-map <MAP>  string as mapp with <count>:<error> delimiter ; 
                         sample: "4:500,6:400"
                         responed with error on every 4th call with 500 and every 6th call with 400 [env: HTTP_DUMP_ERROR_MAP=] [default: ]
  -t, --tracelog <LOG>   logging definition [env: HTTP_DUMP_TRACELOG=] [default: info,tower=info]
  -h, --help             Print help
  -V, --version          Print version
``` 

### Error response usage

It is possible return errors on requests on counter basis. This means an defined error is returned on multiple of the count number and the request number.

A sample. By starting with 

```sh
http-dump -e "6:400;7:500"
```
* on every multiple request of 6 (6, 12, 18, 24, ...) an response error of 400 is thrown.
* on every multiple request of 7 (7, 14, 21, 28, ...) an response error of 500 is thrown.

### Server

You can bind the server with the --bind options to any host or port you want.
The server is path agnostic, every path will be dumped.

## Docker, Podman

Deploy and run inside of Docker or Podman is possible.

Building the image
```sh
docker build -t nbis-dev/http-dump .   
```
Run the image
```sh
docker run -p 127.0.0.1:8089:8089/tcp nbis-dev/http-dump
```

 The options can be passed via environment variables with the `-e` or `--env` flag or from an `.env` file with the `--en-file` option. 

```sh
docker run -e HTTP_DUMP_ERROR_MAP="6:400;7:500"
podman run -e HTTP_DUMP_ERROR_MAP="6:400;7:500"

echo 'HTTP_DUMP_ERROR_MAP="6:400;7:500"' > http-dump.env
docker run --env-file http-dump.env
podman run --env-file http-dump.env
```

## License

http-dump is licensed under [Apache 2.0](https://www.apache.org/licenses/LICENSE-2.0)
