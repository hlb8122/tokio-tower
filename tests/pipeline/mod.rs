use crate::{ready, unwrap, EchoService, PanicError, Request, Response};
use async_bincode::*;
use futures_util::pin_mut;
use tokio;
use tokio::net::{TcpListener, TcpStream};
use tokio_tower::pipeline::{Client, Server};
use tower_service::Service;
use tower_test::mock;

mod client;

#[tokio::test]
async fn integration() {
    let mut rx = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = rx.local_addr().unwrap();

    // connect
    let tx = TcpStream::connect(&addr).await.unwrap();
    let tx: AsyncBincodeStream<_, Response, _, _> = AsyncBincodeStream::from(tx).for_async();
    let mut tx: Client<_, PanicError, _> = Client::new(tx);

    // accept
    let (rx, _) = rx.accept().await.unwrap();
    let rx = AsyncBincodeStream::from(rx).for_async();
    let server = Server::new(rx, EchoService);

    tokio::spawn(async move { server.await.unwrap() });

    unwrap(ready(&mut tx).await);
    let fut1 = tx.call(Request::new(1));
    unwrap(ready(&mut tx).await);
    let fut2 = tx.call(Request::new(2));
    unwrap(ready(&mut tx).await);
    let fut3 = tx.call(Request::new(3));
    unwrap(fut1.await).check(1);
    unwrap(fut2.await).check(2);
    unwrap(fut3.await).check(3);
}

#[tokio::test]
async fn racing_close() {
    let mut rx = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = rx.local_addr().unwrap();

    // connect
    let tx = TcpStream::connect(&addr).await.unwrap();
    let tx: AsyncBincodeStream<_, Response, _, _> = AsyncBincodeStream::from(tx).for_async();
    let mut tx: Client<_, PanicError, _> = Client::new(tx);

    let (service, handle) = mock::pair::<Request, Response>();
    pin_mut!(handle);

    // accept
    let (rx, _) = rx.accept().await.unwrap();
    let rx = AsyncBincodeStream::from(rx).for_async();
    let server = Server::new(rx, service);

    tokio::spawn(async move { server.await.unwrap() });

    // we now want to set up a situation where a request has been sent to the server, and then the
    // client goes away while the request is still outstanding. in this case, the connection to the
    // server will be shut down in the write direction, but not in the read direction.

    // send a couple of request
    unwrap(ready(&mut tx).await);
    let fut1 = tx.call(Request::new(1));
    unwrap(ready(&mut tx).await);
    let fut2 = tx.call(Request::new(2));
    unwrap(ready(&mut tx).await);
    // drop client to indicate no more requests
    drop(tx);
    // respond to both requests one after the other
    // the response to the first should trigger the state machine to handle
    // a read after it has poll_closed on the transport.
    let (req1, rsp1) = handle.as_mut().next_request().await.unwrap();
    req1.check(1);
    rsp1.send_response(Response::from(req1));
    unwrap(fut1.await).check(1);
    let (req2, rsp2) = handle.as_mut().next_request().await.unwrap();
    req2.check(2);
    rsp2.send_response(Response::from(req2));
    unwrap(fut2.await).check(2);
}
