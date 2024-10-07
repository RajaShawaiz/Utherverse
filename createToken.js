const { percentAmount, generateSigner, signerIdentity, createSignerFromKeypair, TransactionBuilder } =require( '@metaplex-foundation/umi');
const { setComputeUnitLimit, setComputeUnitPrice} =require('@metaplex-foundation/mpl-toolbox');
const { TokenStandard, createAndMint } =require( '@metaplex-foundation/mpl-token-metadata');
const { createUmi } =require( '@metaplex-foundation/umi-bundle-defaults');
const { mplCandyMachine } =require("@metaplex-foundation/mpl-candy-machine");

const umi = createUmi('https://api.mainnet-beta.solana.com'); //Replace with your Helius RPC Endpoint
// const umi = createUmi('https://api.devnet.solana.com'); //Replace with your Helius RPC Endpoint

console.log("🚀 ~ umi:", umi);

const userWallet = umi.eddsa.createKeypairFromSecretKey(Buffer.from(process.env.SECRET_KEY));
console.log("🚀 ~ userWallet:", userWallet);
 
const userWalletSigner = createSignerFromKeypair(umi, userWallet);
console.log("🚀 ~ userWalletSigner:", userWalletSigner)

const metadata = {
    name: "Utherverse",
    symbol: "UTHX",
    uri: "https://gold-impressive-krill-904.mypinata.cloud/ipfs/Qmbr7edQjFA7o9Drpc8U9YdBVRvUpN2u9QCyVHWns6SogQ", // Metadata file
};


const mint = generateSigner(umi);
console.log("🚀 ~ mint:", mint);
umi.use(signerIdentity(userWalletSigner));
umi.use(mplCandyMachine())

 createAndMint(umi, {
    mint,
    authority: umi.identity,
    name: metadata.name,
    symbol: metadata.symbol,
    uri: metadata.uri,
    sellerFeeBasisPoints: percentAmount(0),
    decimals: 9,
    amount: 1_000000000n, //1 Token
    tokenOwner: userWallet.publicKey,
    tokenStandard: TokenStandard.Fungible,
    }).add(setComputeUnitLimit(umi, { units: 600_000 })).add(setComputeUnitPrice(umi, { microLamports: 2022000 })).sendAndConfirm(umi).then(() => {
    console.log("Successfully minted 1 UTX token (", mint.publicKey, ")");
});
