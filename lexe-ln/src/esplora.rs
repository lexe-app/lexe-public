use esplora_client::AsyncClient;

pub struct LexeEsplora(AsyncClient);

impl LexeEsplora {
    pub fn new(inner: AsyncClient) -> Self {
        Self(inner)
    }
}
