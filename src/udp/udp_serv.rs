use super::error;
use net2::{UdpBuilder, UdpSocketExt};
use std::convert::TryFrom;
use std::error::Error;
use std::future::Future;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use tokio::net::udp::{RecvHalf, SendHalf};
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio::time::{delay_for, Duration, Instant};


#[cfg(not(target_os = "windows"))]
use net2::unix::UnixUdpBuilderExt;



/// UDP 单个包最大大小,默认4096 主要看MTU一般不会超过1500在internet 上
/// 如果局域网有可能大点,4096一般够用.
pub const BUFF_MAX_SIZE: usize = 4096;


/// UDP SOCKET 上下文,包含 id, 和Pees, 用于收发数据
pub struct UdpContext {
    pub id: usize,
    recv: Arc<Mutex<RecvHalf>>,
    pub send: Arc<Mutex<SendHalf>>
}

/// 错误输入类型
pub type ErrorInput=Arc<Mutex<dyn Fn(Option<SocketAddr>, Box<dyn Error>) + Send>>;

/// UDP 服务器对象
/// I 用来限制必须input的FN 原型,
/// R 用来限制 必须input的是 异步函数
pub struct UdpServer<I, R, S>
    where
        I: Fn(Arc<S>,Arc<Mutex<SendHalf>>, SocketAddr, Vec<u8>) -> R + Send + Sync + 'static,
        R: Future<Output = Result<(), Box<dyn Error>>>,
        S: Sync +Send+'static
{
    inner:Arc<S>,
    udp_contexts: Vec<UdpContext>,
    input: Option<Arc<I>>,
    error_input: Option<ErrorInput>
}



/// 用来存储Token
#[derive(Debug)]
pub struct TokenStore<T:Send>(pub Option<T>);

impl<T:Send> TokenStore<T>{
    pub fn have(&self)->bool{
        self.0.is_some()
    }

    pub fn get(&mut self)->Option<&mut T> {
        self.0.as_mut()
    }

    pub fn set(&mut self,v:Option<T>) {
        self.0 = v;
    }
}


/// 用来存储SendHalf 并实现Write
#[derive(Debug)]
pub struct UdpSend(pub Arc<Mutex<SendHalf>>,pub SocketAddr);

impl UdpSend{
    pub async fn send(&self,buf: &[u8])->std::io::Result<usize> {
        self.0.lock().await.send_to(buf,&self.1).await
    }
}


impl <I,R> UdpServer<I,R,()>  where
    I: Fn(Arc<()>,Arc<Mutex<SendHalf>>,SocketAddr, Vec<u8>) -> R + Send + Sync + 'static,
    R: Future<Output = Result<(), Box<dyn Error>>> + Send {

    pub async fn new<A: ToSocketAddrs>(addr:A)->Result<Self, Box<dyn Error>> {
        Self::new_inner(addr,Arc::new(())).await
    }
}

impl<I, R, S> UdpServer<I, R, S>
    where
        I: Fn(Arc<S>,Arc<Mutex<SendHalf>>,SocketAddr, Vec<u8>) -> R + Send + Sync + 'static,
        R: Future<Output = Result<(), Box<dyn Error>>> + Send,
        S: Sync +Send + 'static{
    ///用于非windows 创建socket,和windows的区别在于开启了 reuse_port
    #[cfg(not(target_os = "windows"))]
    fn make_udp_client<A: ToSocketAddrs>(addr: &A) -> Result<std::net::UdpSocket, Box<dyn Error>> {
        let res = UdpBuilder::new_v4()?
            .reuse_address(true)?
            .reuse_port(true)?
            .bind(addr)?;
        Ok(res)
    }



    ///用于windows创建socket
    #[cfg(target_os = "windows")]
    fn make_udp_client<A: ToSocketAddrs>(addr: &A) -> Result<std::net::UdpSocket, Box<dyn Error>> {
        let res = UdpBuilder::new_v4()?.reuse_address(true)?.bind(addr)?;
        Ok(res)
    }

    ///创建udp socket,并设置buffer 大小
    fn create_udp_socket<A: ToSocketAddrs>(
        addr: &A,
    ) -> Result<std::net::UdpSocket, Box<dyn Error>> {
        let res = Self::make_udp_client(addr)?;
        res.set_send_buffer_size(1784 * 10000)?;
        res.set_recv_buffer_size(1784 * 10000)?;
        Ok(res)
    }

    /// 创建tokio的udpsocket ,从std 创建
    fn create_async_udp_socket<A: ToSocketAddrs>(addr: &A) -> Result<UdpSocket, Box<dyn Error>> {
        let std_sock = Self::create_udp_socket(&addr)?;
        let sock = UdpSocket::try_from(std_sock)?;
        Ok(sock)
    }

    /// 创建tikio UDPClient
    /// listen_count 表示需要监听多少份的UDP SOCKET
    fn create_udp_socket_list<A: ToSocketAddrs>(
        addr: &A,
        listen_count: usize,
    ) -> Result<Vec<UdpSocket>, Box<dyn Error>> {
        println!("cpus:{}", listen_count);
        let mut listens = vec![];
        for _ in 0..listen_count {
            let sock = Self::create_async_udp_socket(addr)?;
            listens.push(sock);
        }
        Ok(listens)
    }

    #[cfg(not(target_os = "windows"))]
    fn get_cpu_count() -> usize {
        num_cpus::get()
    }

    #[cfg(target_os = "windows")]
    fn get_cpu_count() -> usize {
        1
    }

    /// 创建UdpServer
    /// 如果是linux 是系统,他会根据CPU核心数创建等比的UDP SOCKET 监听同一端口
    /// 已达到 M级的DPS 数量
    pub async fn new_inner<A: ToSocketAddrs>(addr: A, inner:Arc<S>) -> Result<Self, Box<dyn Error>> {
        let udp_list = Self::create_udp_socket_list(&addr, Self::get_cpu_count())?;

        let mut udp_map = vec![];

        let mut id = 1;
        for udp in udp_list {
            let (recv, send) = udp.split();
            udp_map.push(UdpContext {
                id,
                recv: Arc::new(Mutex::new(recv)),
                send: Arc::new(Mutex::new( send))
            });
            id += 1;
        }

        Ok(UdpServer {
            inner,
            udp_contexts: udp_map,
            input: None,
            error_input: None
        })
    }



    /// 设置收包函数
    /// 此函数必须符合 async 模式
    pub fn set_input(&mut self, input: I) {
        self.input = Some(Arc::new(input));
    }

    /// 设置错误输出
    /// 返回bool 如果 true　表示停止服务
    pub fn set_err_input<P: Fn(Option<SocketAddr>, Box<dyn Error>)+ Send + 'static>(&mut self, err_input: P) {
        self.error_input = Some(Arc::new(Mutex::new(err_input)));
    }


    /// 启动服务
    /// 如果input 发生异常,将会发生错误
    /// 这个时候回触发 err_input, 如果没有使用 set_err_input 设置错误回调
    /// 那么 就会输出默认的 err_input,如果输出默认的 err_input 那么整个服务将会停止
    /// 所以如果不想服务停止,那么必须自己实现 err_input 并且返回 false
    pub async fn start(&self) -> Result<(), Box<dyn Error>> {
        if let Some(input) = &self.input {
            let mut tasks = vec![];
            for udp_sock in self.udp_contexts.iter() {
                let recv_sock = Arc::downgrade(&udp_sock.recv);
                let send_sock = udp_sock.send.clone();
                let input=input.clone();
                let inner= self.inner.clone();
                let err_input = {
                    if let Some(err) = &self.error_input {
                        let x = err;
                        x.clone()
                    } else {
                        Arc::new(Mutex::new(|addr:Option<SocketAddr>, err:Box<dyn Error>| {
                            match addr {
                                Some(addr) => {
                                    println!("{}-{}", addr, err);
                                }
                                None => {
                                    println!("{}", err);
                                }
                            }
                        }))
                    }
                };

                let pd = tokio::spawn(async move {
                    let wk = recv_sock.upgrade();
                    if let Some(sock_mutex) = wk {
                        let mut buff = [0; BUFF_MAX_SIZE];
                        loop {
                            let res = {
                                let mut sock = sock_mutex.lock().await;
                                sock.recv_from(&mut buff).await
                            };

                            if let Ok((size, addr)) = res {
                                if size>0 {
                                    let next_input = input.clone();
                                    let next_inner = inner.clone();
                                    let next_send_sock = send_sock.clone();
                                    let next_err_input = err_input.clone();
                                    let start = Instant::now();
                                    tokio::spawn(async move {
                                        let res = next_input(next_inner, next_send_sock, addr, buff[0..size].to_vec()).await;
                                        if let Err(er) = res {
                                            let msg = format!("{}", er);
                                            let error = next_err_input.try_lock();
                                            if let Ok(error) = error {
                                                error(
                                                    Some(addr),
                                                    msg.into(),
                                                );
                                            }
                                        }
                                    });
                                }

                            } else if let Err(er) = res {
                                let error = err_input.lock().await;
                                error(None, error::Error::IOError(er).into());
                            }
                        }
                    } else {
                        delay_for(Duration::from_millis(1)).await;
                    }
                });
                tasks.push(pd);
            }

            for task in tasks {
                task.await?;
            }

            Ok(())
        } else {
            panic!("not found input")
        }
    }

}
