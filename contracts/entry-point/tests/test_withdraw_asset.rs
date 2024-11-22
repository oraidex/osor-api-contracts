use cosmwasm_std::{
    testing::{mock_dependencies, mock_env, mock_info},
    Addr, BankMsg, Coin, CosmosMsg, SubMsg, Uint128,
};
use skip::{asset::Asset, entry_point::ExecuteMsg, swap::SwapVenue};
use skip_api_entry_point::{
    error::ContractError,
    state::{BLOCKED_CONTRACT_ADDRESSES, IBC_TRANSFER_CONTRACT_ADDRESS, OWNER, SWAP_VENUE_MAP},
};
use test_case::test_case;

/*
Test Cases:

Expect Response
    - Happy Path (tests the adapter and blocked contract addresses are stored correctly)

Expect Error:
    - Unauthorized
Expect Error
    - Duplicate Swap Venue Names
 */

// Define test parameters
struct Params {
    owner: Option<Addr>,
    coin: Asset,
    sender: String,
    expected_error: Option<ContractError>,
    expected_msgs: Vec<SubMsg>,
}

// Test instantiate
#[test_case(
    Params {
        owner: None,
        coin: Asset::Native(Coin::new(1_000_000, "os")),
        sender: "creator".to_string(),
        expected_error: None,
        expected_msgs: vec![SubMsg::new(CosmosMsg::Bank(BankMsg::Send {
                    to_address: "creator".to_string(),
                    amount: vec![Coin {
                        denom: "os".to_string(),
                        amount: Uint128::new(1000000)
                    }]
                }))]
    };
    "Happy Path")]
#[test_case(
    Params {
        owner: None,
        coin: Asset::Native(Coin::new(1_000_000, "os")),
         sender: "sender".to_string(),
        expected_error: Some(ContractError::AdminError(cw_controllers::AdminError::NotAdmin{})),
        expected_msgs: vec![]
    };
    
    "Unauthorized")]
fn test_withdraw_asset(params: Params) {
    // Create mock dependencies
    let mut deps = mock_dependencies();

    // Create mock info
    let info = mock_info(&params.sender, &[]);

    // Create mock env with the entry point contract address
    let mut env = mock_env();
    env.contract.address = Addr::unchecked("entry_point");

    // init owner
    OWNER
        .set(deps.as_mut(), Some(Addr::unchecked("creator")))
        .unwrap();

    // Call instantiate with the given test parameters
    let res = skip_api_entry_point::contract::execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::WithdrawAsset {
            coin: params.coin,
            receiver: None,
        },
    );

    match res {
        Ok(res) => {
            // Assert the test did not expect an error
            assert!(
                params.expected_error.is_none(),
                "expected test to error with {:?}, but it succeeded",
                params.expected_error
            );
            assert_eq!(
                res.messages,
                params.expected_msgs
            );

            // // Get stored ibc transfer adapter contract address
            // let stored_ibc_transfer_contract_address = IBC_TRANSFER_CONTRACT_ADDRESS
            //     .load(deps.as_ref().storage)
            //     .unwrap();

            // // Assert the ibc transfer adapter contract address exists in the blocked contract addresses map
            // assert!(BLOCKED_CONTRACT_ADDRESSES
            //     .has(deps.as_ref().storage, &stored_ibc_transfer_contract_address));

            // params.swap_venues.into_iter().for_each(|swap_venue| {
            //     // Get stored swap venue adapter contract address
            //     let stored_swap_venue_contract_address = SWAP_VENUE_MAP
            //         .may_load(deps.as_ref().storage, &swap_venue.name)
            //         .unwrap()
            //         .unwrap();

            //     // Assert the swap venue name exists in the map and that
            //     // the adapter contract address stored is correct
            //     assert_eq!(
            //         &stored_swap_venue_contract_address,
            //         &Addr::unchecked(&swap_venue.adapter_contract_address)
            //     );

            //     // Assert the swap adapter contract address exists in the blocked contract addresses map
            //     assert!(BLOCKED_CONTRACT_ADDRESSES
            //         .has(deps.as_ref().storage, &stored_swap_venue_contract_address));
            // });
        }
        Err(err) => {
            // Assert the test expected an error
            assert!(
                params.expected_error.is_some(),
                "expected test to succeed, but it errored with {:?}",
                err
            );

            // Assert the error is correct
            assert_eq!(err, params.expected_error.unwrap());
        }
    }
}
