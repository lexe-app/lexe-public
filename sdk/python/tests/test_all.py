import pytest
import lexe

def test_add():
    assert lexe.add(1, 2) == 3

@pytest.mark.skip(reason="Requires network access")
async def test_gateway_client_latest_enclave():
    deploy_env = lexe.DeployEnv.STAGING
    gateway_url = "https://lexe-staging-sgx.uswest2.staging.lexe.app"
    user_agent = "sdk-python/0.1.0"
    client = lexe.GatewayClient(deploy_env, gateway_url, user_agent)
    enclave = await client.latest_enclave()

    print("enclave:", enclave)
    assert enclave != ""
