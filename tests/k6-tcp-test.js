// require https://github.com/NAlexandrov/xk6-tcp
import tcp from 'k6/x/tcp';
import { check } from 'k6';

const conn = tcp.connect('127.0.0.1:8080');

export default function () {
  tcp.writeLn(conn, 'Say Hello');
  let res = String.fromCharCode(...tcp.read(conn, 1024))
  check (res, {
    'verify ag tag': (res) => res.includes('Hello')
  });
  tcp.close(conn);
}
