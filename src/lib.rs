use near_contract_standards::non_fungible_token::metadata::{
    NFTContractMetadata, NonFungibleTokenMetadataProvider, TokenMetadata, NFT_METADATA_SPEC,
};

use near_contract_standards::non_fungible_token::{Token, TokenId};
use near_contract_standards::non_fungible_token::NonFungibleToken;

use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LazyOption, UnorderedSet};
use near_sdk::json_types::ValidAccountId;
use near_sdk::{
    env, near_bindgen, AccountId, BorshStorageKey, PanicOnDefault, Promise, PromiseOrValue, Balance
};

near_sdk::setup_alloc!();

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct Contract {
    tokens: NonFungibleToken,
    metadata: LazyOption<NFTContractMetadata>,
    //custom
    current_token_id: TokenId,
    price: Balance,
}

#[derive(BorshSerialize, BorshStorageKey)]
enum StorageKey {
    NonFungibleToken,
    Metadata,
    TokenMetadata,
    Enumeration,
    Approval,
    //custom
    TokensPerOwner { account_hash: Vec<u8> },
}

#[near_bindgen]
impl Contract {

    #[init]
    pub fn new_default_meta(owner_id: ValidAccountId) -> Self {
        Self::new(
            owner_id,
            NFTContractMetadata {
                spec: NFT_METADATA_SPEC.to_string(),
                name: "FEEEEM NFT V1".to_string(),
                symbol: "FEEM".to_string(),
                icon: None,
                base_uri: Some("https://gateway.pinata.cloud/ipfs/QmZxPLVrYwbXLiDaqUMo9ewjm8FXQ4ci5SPBCfab3RYcEb/".to_string()),
                reference: None,
                reference_hash: None,
            },
        )
    }

    #[init]
    pub fn new(owner_id: ValidAccountId, metadata: NFTContractMetadata) -> Self {
        assert!(!env::state_exists(), "Already Initialized");
        metadata.assert_valid();
        Self {
            tokens: NonFungibleToken::new(
                StorageKey::NonFungibleToken, 
                owner_id,
                Some(StorageKey::TokenMetadata),
                Some(StorageKey::Enumeration),
                Some(StorageKey::Approval),
            ),
            metadata: LazyOption::new(StorageKey::Metadata, Some(&metadata)),
            current_token_id: "0".to_string(),
            //5 Near
            price: 5_000_000_000_000_000_000_000_000,
        }
    }


    fn nft_mint(
        &mut self,
        token_id: TokenId,
        receiver_id: ValidAccountId,
        token_metadata: TokenMetadata,
    ) -> Token {
        //@notice old implementation is limit for only owner could mint the nft
        // self.tokens.mint(token_id, receiver_id, Some(token_metadata))
        let metadata = Some(token_metadata);
        if self.tokens.token_metadata_by_id.is_some() && metadata.is_none() {
            env::panic(b"Must provide metadata");
        }
        if self.tokens.owner_by_id.get(&token_id).is_some() {
            env::panic(b"token_id must be unique");
        }

        let owner_id: AccountId = receiver_id.into();

        // Core behavior: every token must have an owner
        self.tokens.owner_by_id.insert(&token_id, &owner_id);

        // Metadata extension: Save metadata, keep variable around to return later.
        // Note that check above already panicked if metadata extension in use but no metadata
        // provided to call.
        self.tokens.token_metadata_by_id
            .as_mut()
            .and_then(|by_id| by_id.insert(&token_id, &metadata.as_ref().unwrap()));

        // Enumeration extension: Record tokens_per_owner for use with enumeration view methods.
        if let Some(tokens_per_owner) = &mut self.tokens.tokens_per_owner {
            let mut token_ids = tokens_per_owner.get(&owner_id).unwrap_or_else(|| {
                UnorderedSet::new(StorageKey::TokensPerOwner {
                    account_hash: env::sha256(owner_id.as_bytes()),
                })
            });
            token_ids.insert(&token_id);
            tokens_per_owner.insert(&owner_id, &token_ids);
        }

        // Approval Management extension: return empty HashMap as part of Token
        let approved_account_ids =
            if self.tokens.approvals_by_id.is_some() { Some(HashMap::new()) } else { None };


        Token { token_id, owner_id, metadata, approved_account_ids }
    }

    fn mint_one(&mut self, receiver_id: ValidAccountId) -> Token {
        let current_token_id = self.get_current_token_id();
        let metadata = self.generate_metadata(current_token_id.clone());
        let token = self.nft_mint(current_token_id.clone(), receiver_id.clone(), metadata);
        self.increase_token_id();
        return token;
    }

    fn increase_token_id(&mut self) {
        let mut token_id: u16 = self.current_token_id.parse().unwrap();
        token_id +=1;
        self.current_token_id = token_id.to_string();
    }

    fn get_current_token_id(&self) -> TokenId {
        let mut token_id: u16 = self.current_token_id.parse().unwrap();
        token_id +=1;
        token_id.to_string()
    }

    fn generate_metadata(&self, token_id: TokenId) -> TokenMetadata {
        let token_to_title = format!("Feem NFT #{}", token_id.to_string());
        let base_uri = self.metadata.get().unwrap().base_uri.unwrap();
        let token_uri = format!("{}{}.png",base_uri.to_string(), token_id.to_string());
        TokenMetadata {
            title: Some(token_to_title.into()),
            description: Some("My FIRST NFT".into()),
            media: Some(token_uri),
            media_hash: None,
            copies: Some(1u64),
            issued_at: Some(env::block_timestamp().to_string()),
            expires_at: None,
            starts_at: None,
            updated_at: None,
            extra: None,
            reference: None,
            reference_hash: None,
        }
    }

    #[payable]
    pub fn buy_nft_one(&mut self, receiver_id: ValidAccountId) -> Token {
        let initial_storage_usage = env::storage_usage();
        let price: u128 = self.price;
        let attached_deposit = env::attached_deposit();
        assert!(
            attached_deposit >= price,
            "MEEF NFT: attached deposit is less than price : {}",
            price
        );
        let token = self.mint_one(receiver_id.clone());
        Promise::new(self.tokens.owner_id.to_string()).transfer(price);
        refund_deposit(env::storage_usage() - initial_storage_usage, price);
        return token;
    }

    pub fn get_owner(&self) -> AccountId {
        self.tokens.owner_id.to_string()
    }
}

fn refund_deposit(storage_used: u64, extra_spend: Balance) {
    let required_cost = env::storage_byte_cost() * Balance::from(storage_used);
    let attached_deposit = env::attached_deposit() - extra_spend;

    assert!(
        required_cost <= attached_deposit,
        "Must attach {} yoctoNEAR to cover storage",
        required_cost,
    );

    let refund = attached_deposit - required_cost;
    if refund > 1 {
        Promise::new(env::predecessor_account_id()).transfer(refund);
    }
}

near_contract_standards::impl_non_fungible_token_core!(Contract, tokens);
near_contract_standards::impl_non_fungible_token_approval!(Contract, tokens);
near_contract_standards::impl_non_fungible_token_enumeration!(Contract, tokens);

#[near_bindgen]
impl NonFungibleTokenMetadataProvider for Contract {
    fn nft_metadata(&self) -> NFTContractMetadata {
        self.metadata.get().unwrap()
    }
}


#[cfg(all(test, not(target_arch = "wasm32")))]
mod test {
    use near_sdk::MockedBlockchain;
    use near_sdk::test_utils::{accounts, VMContextBuilder};
    use near_sdk::testing_env;

    use super::*;

    const MINT_STORAGE_COST: u128 = 9_000_000_000_000_000_000_000;

    fn get_context(predecessor_account_id: ValidAccountId) -> VMContextBuilder {
        let mut builder = VMContextBuilder::new();
        builder.current_account_id(accounts(0))
        .signer_account_id(predecessor_account_id.clone())
        .predecessor_account_id(predecessor_account_id);
        builder
    }

    fn test_token_metadata(token_id: TokenId) -> TokenMetadata {
        let token_to_title = format!("Feem NFT #{}", token_id.to_string());
        let token_uri = format!("https://gateway.pinata.cloud/ipfs/{}.png",token_id.to_string());
        TokenMetadata {
            title: Some(token_to_title.into()),
            description: Some("My FIRST NFT".into()),
            media: Some(token_uri),
            media_hash: None,
            copies: Some(1u64),
            issued_at: Some(env::block_timestamp().to_string()),
            expires_at: None,
            starts_at: None,
            updated_at: None,
            extra: None,
            reference: None,
            reference_hash: None,
        }
    }

    #[test]
    fn test_new() {
        let mut context = get_context(accounts(1));
        testing_env!(context.build());
        let contract = Contract::new_default_meta(accounts(1).into());
        testing_env!(context.is_view(true).build());
        assert_eq!(contract.nft_token("1".to_string()), None);
    }

    #[test]
    fn test_mint() {
        let mut context = get_context(accounts(0));
        testing_env!(context.build());
        let mut contract = Contract::new_default_meta(accounts(0).into());
        let price: Balance = 5_000_000_000_000_000_000_000_000; 

        testing_env!(context
            .storage_usage(env::storage_usage())
            .attached_deposit(MINT_STORAGE_COST + price)
            .predecessor_account_id(accounts(0))
            .build()
        );

        let token_id_1 = "1".to_string();
        let token_id_2 = "2".to_string();
        let token1 = contract.buy_nft_one(accounts(1));
        let token2 = contract.buy_nft_one(accounts(1));
        let total_supply = contract.nft_total_supply();
        
        {
            println!("storage usage: {:?}", env::storage_usage());
            println!("metadata 1: {:?}", token1.metadata);
            println!("metadata 2: {:?}", token2.metadata);
            println!("total supply: {:?}", total_supply);
        }

        assert_eq!(token1.token_id, token_id_1);
        assert_eq!(token1.owner_id, accounts(1).to_string());
        assert_eq!(token2.token_id, token_id_2);
        assert_eq!(token2.owner_id, accounts(1).to_string());
        assert_eq!(token1.metadata.unwrap(), test_token_metadata(token_id_1));
        assert_eq!(token2.metadata.unwrap(), test_token_metadata(token_id_2));
        assert_eq!(token1.approved_account_ids.unwrap(), HashMap::new()); 
        assert_eq!(token2.approved_account_ids.unwrap(), HashMap::new()); 
    }

    #[test]
    fn test_transfer() {
        let mut context = get_context(accounts(0));
        testing_env!(context.build());
        let mut contract = Contract::new_default_meta(accounts(0).into());
        let price: Balance = 5_000_000_000_000_000_000_000_000; 

        testing_env!(context
            .storage_usage(env::storage_usage())
            .attached_deposit(MINT_STORAGE_COST + price)
            .predecessor_account_id(accounts(0))
            .build()
        );

        let token_id1 = "1".to_string();
        let token_id2 = "2".to_string();
        contract.buy_nft_one(accounts(0));
        contract.buy_nft_one(accounts(0));

        testing_env!(context
            .storage_usage(env::storage_usage())
            .attached_deposit(1)
            .predecessor_account_id(accounts(0))
            .build()
        );

        contract.nft_transfer(accounts(1), token_id1.clone(), None, None);
        contract.nft_transfer(accounts(2), token_id2.clone(), None, None);

        testing_env!(context
            .storage_usage(env::storage_usage())
            .account_balance(env::account_balance())
            .is_view(true)
            .attached_deposit(0)
            .build()
        );

        let token1 = contract.nft_tokens_for_owner(accounts(1), Some(U128(0)), None);
        let token2 = contract.nft_tokens_for_owner(accounts(2), Some(U128(0)), None);

        if token1.len() > 0 && token2.len() > 0 {
            assert_eq!(token1[0].token_id, token_id1);
            assert_eq!(token2[0].token_id, token_id2);
            assert_eq!(token1[0].owner_id, accounts(1).to_string());
            assert_eq!(token2[0].owner_id, accounts(2).to_string());
        } else {
            panic!("token not correctly created. or not found by nft_tokens_for_owner");
        }
    }
}