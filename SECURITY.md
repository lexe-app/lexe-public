# Lexe Security Model and Responsible Disclosure

Lexe takes security very seriously.
The following outlines Lexe's disclosure process and security model.

## Responsible Disclosure

If you find a security vulnerability that affects Lexe or Lexe's users, we would deeply appreciate following a responsible disclosure process.
Please email your report to `disclosure@lexe.tech`, after which we will review your report, attempt to reproduce the vulnerability, and try our best to release a patch for the vulnerability within 90 days.
We will keep you updated on the status of the vulnerability as we work through steps to address it.

Lexe does not have a standardized bug bounty program yet, but we will do our best to compensate you accordingly, especially for more serious vulnerabilities.

## Adversaries

We define several different classes of adversaries which could try to compromise user funds:

### Blackhat Brian

Brian has access to everything a typical Lexe user does, including Lexe's public source code and the ability to build and run a malicious client instead of using Lexe's versions.
Brian can make arbitrary network requests to Lexe's public services to try to steal the funds of other users.
Brian can run a Lightning node externally using the same seed used by their Lexe node, as well as double spend unconfirmed on-chain transactions.

### Eavesdropper Eve

In addition to all of Brian's capabilities, Eve can read network messages sent to and from Lexe's servers over the wire.
Eve does not have access to Lexe's cloud instances.

### Message malleator Mallory

In addition to Eve's capabilities, Mallory can tamper with network messages sent to and from Lexe's servers over the wire.

### Rogue employee Raven

In addition to Mallory's capabilities, Raven has access to Lexe's cloud instances, where the Lexe's database is stored and where users' Lightning nodes run.
Raven has access to Lexe's LSP, and can broadcast old channel states in order to attempt to steal funds from Lexe users.
Raven can prevent a node from running at all, run a node as many times as she wants (including in parallel, with command line inputs of her choice), or stop a node at an arbitrary point during its execution.
Using these powers, Raven can attempt side-channel attacks on users' nodes with the goal of retrieving sensitive information from within SGX. Raven also has access to Lexe's software signing key.

Although we would never want to risk our hard-earned trust and reputation by attacking our own users, for Lexe to qualify as non-custodial, we must demonstrate that Lexe users are reasonably secure even in the presence of a Raven-like adversary.
Security-conscious users who take the time to read through Lexe's source code will notice that user nodes have been carefully designed to protect even against this threat model.

## Threat Models

The following sections discuss the different attack vectors relevant to Lexe users, altogether constituting the bulk of Lexe's security model.
One concept used regularly throughout this section is that of the **security-conscious user**, i.e. a Lexe user who wishes to adhere to the "don't trust, verify" ethos and place as little trust into Lexe as possible.
Due to the technical difficulty of verifying all aspects of Lexe's security, we do not expect the majority of Lexe users to carry out all verification steps, which includes manual steps in addition to programmatic ones.
It is much more likely that the average user only conducts the verification that is done programmatically by their mobile clients, explicitly or implicitly delegating the full verification of Lexe's claims to someone or a group of people that they trust.

### Code integrity and secret provisioning

**How do I know that Lexe is running the open-source code in the `lexe-public` repo, instead of some malicious program intended to extract my private key?**

This is an important question to ask, and one that is answered in multiple steps.

First, the Lightning nodes that run inside SGX have a **reproducible build**, meaning that any user who wishes to do so can start with the source code that Lexe makes publicly available, and through a sequence of deterministic steps, is able to produce the exact same binary that Lexe signs and runs in production.
More precisely, the user needs to verify the **measurement** of the program, which is an SGX term for the SHA256 hash of the production binary and the program's initial memory contents.
As part of this process, the user checks that the binary is indeed the release build, and that test components which have the possibility of exposing sensitive information about the program are compiled out.

After a user has verified the measurement, how do they know that Lexe is indeed running their Lightning node with this specific measurement? This is the role of **remote attestation**.
Essentially, the CPU running the Lightning node generates a "proof" that (1) the program matches the expected measurement, (2) the program is running inside SGX, (3) the CPU the program is running on was manufactured by Intel, and (4) the CPU is running the most recent security patch.
The user's mobile client can then **verify** this attestation quote, which is embedded inside a TLS certificate, before provisioning any sensitive information (such as their root seed) into the enclave via the secure channel established using this TLS certificate.

We do not expect the average Lexe user to carry out the complicated technical steps required to reproduce our node builds.
However, so long as there are a sufficient number of independent parties that regularly verify node builds as they are released and publicly attest to them, non-technical users can have reasonable assurance that Lexe is running the expected code.
Particularly security-conscious users should still do the verification themselves, however.
If you are interested in regularly verifying Lexe releases and publicly attesting that the measurements match, please get in touch!

Some additional details:

- Verifying the remote attestation is done programmatically inside the user's mobile app as part of the secret provisioning process. But how does the user know that their mobile app is _actually_ doing the verification, instead of simply accepting all attestations? In short, the mobile apps are open-source and reproducible as well.
- Note that Rogue Raven, who has access to Lexe's software signing key, can release a malicious update. In order to protect against this, a security conscious user must first verify the node build before approving the new measurement inside their mobile app, which begins the secret provisioning step for the new measurement. Due to how key derivation for SGX sealing works, secrets provisioned into an old measurement are not accessible to a new program with a different measurement unless the secret is re-provisioned into the new program, so a malicious update which the user does not approve is not able to access any sensitive information previously provisioned into the enclave.
- Users should be aware that app registries such as Google Play or the App Store may contain malicious client software if Lexe's software signing key has been compromised or if the registry itself has been compromised. Security-conscious users should verify their mobile client builds or simply run their own mobile build from source.

### Sealing the provisioned root seed

During the secret provisioning process, the user's **root seed** is provisioned inside the enclave.
The root seed is the secret from which all other secrets are derived, including the ed25519 user keypair used to authenticate requests to Lexe services, the secp256k1 keypair used in the Lightning node, the BIP39 keypairs for holding on-chain Bitcoin, and the AES-GCM keys for encrypting persisted data.
In order for this root seed to be available the next time the user's node runs, the root seed is **sealed**, meaning that it is encrypted under a key partially derived from the enclave hardware which ensures that the root seed can be unsealed only by the specific CPU and measurement that sealed it.
In other words, the root seed cannot be unsealed on a different machine running a SGX program with the same measurement (it would need to be re-provisioned), nor can it be unsealed by a different program on the same machine (which may be malicious software designed to extract the key out of the enclave).

The sealed root seed can thus be safely persisted in Lexe's DB, which the node accesses and unseals the next time it runs.

### Secure communication with the enclave

All communication between the user's mobile app and the user's Lightning node is done over TLS, which guarantees confidentiality, integrity, and authentication in the presence of Eve-, Mallory-, and Raven-type adversaries.

During the secret provisioning process, the user's mobile client connects using a TLS certificate presented by the node.
Authentication is achieved by verifying the remote attestation quote embedded within this initial TLS certificate.

After the initial secret provisioning has been completed, the communication over TLS is conducted using a shared secret derived from the root seed.
Authentication is achieved because (aside from the user's mobile app) only a previously validated and provisioned enclave could have access to the root seed used to establish the TLS connection.

### Authenticating requests made to user nodes

Each user's mobile client interacts with their node via a REST API in order to issue commands such as generating invoices and sending payments.
Suppose that Blackhat Brian finds a vulnerability which allows him to steal a victim's money so long as he can make a request to the victim's node.
How do we prevent him from stealing the funds of all Lexe users?

The answer is that user nodes are not exposed to the public internet.
Instead, mobile clients must communicate with user nodes via Lexe's reverse proxy, which is one of Lexe's few public-facing endpoints.
In order to make a request to a node, a requestor must first authenticate themselves by passing a challenge-response which proves to Lexe that they actually own the node they wish to make requests to.
If the requestor passes the challenge-response, they receive an auth token signed by Lexe which they must then include in their requests to Lexe's reverse proxy which they wish to have forwarded to their node.
When Lexe's reverse proxy receives a request, it first verifies that the auth token was indeed signed by Lexe and has not expired, and only then is the connection forwarded to the intended user node.
Blackhat Brian would not have access to the root seed of his targeted user, and thus would not be able to pass the challenge-response in order to obtain the token required to launch their attack.

Note that in order to prevent Eve-like adversaries from observing the auth tokens, all requests made to Lexe's infrastructure are made over an "outer" TLS connection which terminates at Lexe's reverse proxy.
The proxy itself is prevented from eavesdropping on the requests intended for the user nodes due to the "inner" forwarded connection also being a TLS connection.
The end result is that communication between a user's mobile client and their node is done using TLS-in-TLS.
Man-in-the-middle attacks on the inner TLS connection are prevented due to the authentication mechanisms outlined in the [previous section](#secure-communication-with-the-enclave).

### Confidentiality, integrity and authentication of persisted data

Each user's Lexe node needs to persist some data for normal node operation.
To ensure that this data remains confidential, the sensitive components are encrypted using AES-GCM, an **authenticated encryption** algorithm which ensures that Raven cannot overwrite this data with her own chosen inputs and also have it be accepted by the node the next time it decrypts this data.
The sensitive fields are encrypted under a key derived from the user's root seed and then the ciphertext is stored in Lexe's database along with some metadata.

Not all persisted data is encrypted. Unencrypted metadata includes:

- The user's node's node public key, which is known already by Lexe's LSP, who is the user's sole channel counterparty
- The 'directory' which declares the type of the data stored in the ciphertext, for example whether the ciphertext holds a network graph or a channel scorer.
- The 'filename' for the data, which allows the node to distinguish between multiple values of a given data type. An example 'filename' is the funding txo of each channel to the LSP (which is also already known by Lexe's LSP).
    
This unencrypted metadata allows Lexe's DB to different one user's data from another's, and to be able to retrieve the correct data entries upon request.
See the node's persister for more details.

### Protecting against rollbacks using a 3rd party cloud

One attack vector that SGX explicitly does not protect against is rollbacks or deletions of data persisted outside of the enclave.
Although Raven cannot fool a user's node into accepting data that it did not persist itself, Raven could feed in an older version of the data which the node previously persisted but which has since been invalidated, or refuse to provide the data at all.

Rollback protection is very important for Lightning Network nodes since without it, a malicious Lexe LSP could "roll back" its channel state with the user node to some point in the past where the LSP had a higher balance and cooperatively close the channel with the user's node, effectively stealing funds from the user.

Hence, Lexe's database is treated as an untrusted data store, suitable for storing information which does not compromise user funds if rolled back and which is not entirely critical to the node's operation - examples include payment history and the network graph data.
Critical persisted data which must be secure against rollback attacks (such as channel state) is instead stored in each user's personal cloud storage account, such as in Google Drive or iCloud.

- The persisted data is accessible to the user in the case that they want to close all channels and withdraw all of their funds from their Lexe node using an open-source recovery tool, which does not require the assistance or cooperation of the Lexe company (or even for the Lexe company to exist). The open-source recovery tool spins up a small Lightning node, closes all open channels, and sweeps all funds to an external address specified by the user.
- The persisted data is also programmatically accessible to the user's node running inside the SGX enclave via a cloud storage API, in order to read previously persisted data (i.e. check against rollbacks) as well as persist new updates.
- The _plaintext_ of the persisted data is _not_ accessible to the cloud storage provider, since it is encrypted under a key derived from the user's root seed. The root seed is backed up either as a 24 word mnemonic offline (for advanced users) or is encrypted under a password which the Lexe app regularly quizzes the user on (the more common case).
- Even though the API key for accessing the user's cloud account is provisioned into an enclave running on Lexe's infrastructure, Lexe itself does not have access to any personally-identifiable information which can be obtained from the API key because Lexe does not have access to the API key. In other words, users can connect their personal cloud accounts to their Lexe nodes without revealing any information about themselves (such as the email address used for their cloud account) to the Lexe company.
    

### Denial-of-Service Protection via HTTPS queries and "Security Reports"

Another attack vector that SGX does not prevent is **denial-of-service**.
When a user's node queries the blockchain to sync to the latest chain tip or check for channel breaches, Raven could provide the node with block data corresponding to a false chain, or she could refuse to provide the data at all.

Thus, any block data coming from Lexe's Bitcoin full node must be considered untrusted.
User nodes can still use this data to sync to the latest chain tip, validating the block headers along the way.
However, in order to verify that the synced chain tip indeed corresponds to the heaviest chain, the node will ask a quorum of 3rd party block explorers to confirm.
This API call is done over HTTPS in order to authenticate the 3rd party block explorers and ensure the integrity of the results.
If the HTTPS request fails or the chain tip received does not match, the node shuts down.

User nodes do not undergo the computationally expensive process of validating entire blocks and scanning them for channel breach transactions.
In order to check for channel breaches, the node likewise queries a quorum 3rd party block explorers for information about transactions of interest.

Whenever a node successfully syncs to and validates a chain tip, and completes its check that no channel breach transactions have been broadcasted, it generates and signs a **security report**, which contains information such as the latest block height and chain tip the node synced to, as well as the status of the channels that it is aware of.
In effect, the security report is the node's way of attesting to the user that the Lexe operators have been regularly syncing the user's node and providing it with correct block data, as expected.

The node then stores this security report in the DB.
The next time the user opens their app, their mobile client asks their node to provide a list of its most recent security reports (e.g. all reports generated over the last two weeks).
The user can then verify that the reports have been generated at regular intervals (e.g. at least once a day) as a way to check that their nodes haven't been subjected to a denial-of-service attack.
The user knows that these reports are legitimate because they have been signed by a key derived from the root seed, which only their node inside the enclave (and not Lexe) has access to.

In order for security-conscious users to remain fully secure against denial-of-service attacks which may have been launched to prevent the detection of a channel breach, it is the responsibility of security-conscious users to open their Lexe app at least once every `our_to_self_delay` blocks, specified in the node's `ChannelHandshakeConfig`.
If the user has a channel open, and sees that at least one security report has been generated in the last `our_to_self_delay` blocks, then the user knows that at the present moment their funds are safe, although ideally there should be a report generated at least once every day.

If there are no security reports available, or if they have not been generated frequently enough, the user should immediately use the open-source recovery tool to sweep their funds to an external address.
During this process, the recovery tool will check for and contest any observed channel breaches, thus preventing any loss of funds to a Raven-type adversary.

### Attacks on SGX

Lexe relies extensively on SGX's guarantees with respect to confidentiality, integrity, and remote attestation.
By using SGX (instead of e.g. AMD SEV or not running it inside a TEE at all), we are able to eliminate nearly all other architectural components from within Lexe's trust boundary, including the BIOS / EFI, VMM / hypervisor, operating system kernels, and other tenants in the cloud infrastructure.
All that remains is trust in the CPU manufacturer (who in this case is Intel) which is (with the exception of the exceedingly small number of people who fabricate their own computer chips) an assumption that all mobile, desktop, home server, and cloud users make every day.

That said, just as vulnerabilities are regularly found in all parts of the software stack, from user-facing applications all the way down to operating system kernel, so too are vulnerabilities regularly found in hardware, including the CPU microarchitecture upon which SGX's guarantees are based.

The January 2018 discovery of the Spectre and Meltdown transient execution vulnerabilities, which affected virtually all CPUs, was big news in the security world.
Transient execution-based attacks customized for SGX soon followed, perhaps most notably Foreshadow in 2018.
2019 saw the discovery of Microarchitectural Data Sampling (MDS) based attacks, which has its own list of specific exploits such as Fallout, RIDI, Zombieload, and LVI.
Additional attacks on SGX have been found since, including Plundervolt, a voltage-based side-channel attack, ÆPIC Leak, a microarchitectural bug, and SGAxe.

As is standard for major vulnerabilities such as these, these were responsibly disclosed to the manufacturers (Intel), who had time to release mitigations before the exploits were published.
Typically, existing hardware is patched using microcode updates or changes to the compilers / SDKs used to develop SGX-capable applications.
In some cases, it is up to application, library, or language developers to architect their code in a way that prevents these attacks, such as ensuring that all cryptographic operations are constant time to prevent cache timing attacks, or ensuring that all memory accesses are 8 byte aligned.
Over longer time horizons, however, chip manufacturers are often able to rule out entire classes of attacks entirely via hardware updates and other more fundamental changes to the system design.
For example, in recent generations of Intel CPUs, it is no longer possible to execute a Meltdown-style attack which leaks data from the L1d cache.
Although we can never guarantee that the latest hardware running the latest software will be secure against the next novel attack, relatively new technologies such as SGX have hardened and matured over time, while our tools for proactively identifying and preventing bugs and vulnerabilities continues to improve (e.g. fuzzing, static analysis, memory-safe languages).

**What are the implications for Lexe and Lexe users?**

First, Lexe needs to stay abreast of the latest published vulnerabilities and ensure that known mitigations are enabled in our production deployments.

Lexe needs to stay up to date with all of the latest dependencies, SGX SDKs, drivers, OS patches, BIOS updates, microcode updates, and hardware.
This alone protects Lexe against the vast majority of known vulnerabilities which were responsibly disclosed.
Lexe users should keep their node software up to date by accepting new updates pushed out by the Lexe developers (after verifying them, of course), which provisions their root seed into newer versions.
Over time, Lexe gradually migrates users to instances with newer hardware, which have the ability to address the root cause of known vulnerabilities and implement more comprehensive mitigations by changing the system design at the hardware level, but which still requires users to re-provision.

Lastly, Lexe utilizes defense in depth.
The _key_ observation here is that even if SGX's isolation guarantees have been partially compromised, the primary assets that Lexe needs to protect are the cryptographic keys used inside the node.
More specifically, Lexe can harden the user node software against side channel and transient execution attacks by ensuring that sensitive operations (such as signing data) are conducted by libraries which are constant time.
For example, the library that Lexe uses for ed25519 (`ring`) has been hardened in this regard, and so has the library used for node signatures (`secp256k1`), which requires passing in a random number generator any time a sensitive cryptographic operation is conducted.
What this means is that absent a complete break of SGX's confidentiality or attestation guarantees, Raven will have a very hard time trying to extract sensitive data out of the enclave.
Additionally, we are careful to zeroize memory after keys have been `drop()`ped in order to prevent leaking sensitive information in the case that an attacker has an exploit capable of reading uninitialized memory (such as ÆPIC Leak).

### Intel

As the CPU manufacturer and key participant in the remote attestation process, Intel is a trusted component of any SGX-based system.
It is possible that Intel secretly read the private keys fused onto the CPU die which are used for sealing and unsealing sensitive data as well as for signing attestation quotes, giving Intel the power to read into any enclave and forge attestations at will.
This trust assumption is known to and acknowledged by Lexe and lies outside of the realm of things that Lexe can do something about, and is thus outside of Lexe's security model.

However, we also believe that the "trust Intel" component of SGX receives a disproportionate amount of scrutiny (and dare we say, criticism?) relative to the other trust assumptions we make on a daily basis.
While we too wish that SGX's design was more open, auditable, and easier to reason about, we still chose to go with SGX over the many alternatives (e.g. AMD SEV, Arm TrustZone), because it was the TEE with the greatest maturity and smallest trusted computing base (TCB).

The device you are reading this on most likely contains a chip designed or manufactured by Intel, AMD, or Arm, who could have installed a backdoor that allows them to extract all of your passwords, personal information, and Bitcoin.
You most likely did not fabricate your chips yourself, and even if you did, did you validate that the chip fab you used is secure against a similar supply-chain attack?

You're likely running Windows, macOS, or Linux, and mostly likely did not build your operating system from source (assuming the source is even available) to validate that it does not contain backdoors inserted by the manufacturer of your device.
Even if you did (only possible with Linux), did you read all 30 million+ lines of code in the Linux kernel to ensure that no one inserted a backdoor?

Meanwhile, SGX addresses the "is my operating system actually secure" problem by eliminating the operating system entirely, and tries to squeeze out strong security guarantees using the fewest trust assumptions possible, implementing protections directly in hardware.
One could argue that running a Lightning node inside SGX in the cloud is safer than running it on a Raspberry Pi at home, if only because the TCB is several orders of magnitude smaller.

The point is that we believe "trust Intel" is a reasonable assumption to make, even if only because it's extremely difficult or near impossible to avoid an assumption like it (while still being able to use computers).
We live in a complex society which produces amazing technology in highly interconnected ways, and which currently requires even the most security-conscious users to accept a few trust assumptions which are just too hard to avoid or validate.
We should keep pushing to make every part of our technology stack verifiable, but we also have to work with what we have, today.

### What about homomorphic encryption, multi-party computation, zero-knowledge proofs, etc?

Yes, we have considered the more "crypto-native" approaches such as FHE, MPC, and ZKPs.
While we are big fans of these technologies and believe they will be core components of the future, as they exist today they are nowhere near powerful enough to solve the problems we are focused on: simplifying asynchronous payments and making the Lightning Network much easier to use by running highly-available Lightning nodes on behalf of our users in a non-custodial way.
