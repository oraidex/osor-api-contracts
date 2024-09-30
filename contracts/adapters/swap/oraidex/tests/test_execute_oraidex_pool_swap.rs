use std::vec;

use cosmwasm_std::{
    testing::{mock_dependencies_with_balances, mock_env, mock_info},
    to_json_binary, Addr, Coin, QuerierResult,
    ReplyOn::Never,
    SubMsg, SystemResult, Uint128, WasmMsg, WasmQuery,
};
use cw20::{BalanceResponse, Cw20ExecuteMsg};
use oraiswap::{
    asset::AssetInfo,
    mixed_router::{
        ExecuteMsg as OraidexRouterExecuteMsg,
        SwapOperation as OraidexSwapOperation,
    },
    converter::{ExecuteMsg as ConverterExecuteMsg, Cw20HookMsg as ConverterCw20HookMsg}
};
use skip::swap::{ExecuteMsg, SwapOperation};
use skip_api_swap_adapter_oraidex::{
    error::{ContractError, ContractResult},
    state::{ENTRY_POINT_CONTRACT_ADDRESS, ORAIDEX_ROUTER_ADDRESS},
};
use oraiswap_v3::{percentage::Percentage, FeeTier, PoolKey};
use test_case::test_case;

/*
Test Cases:

Expect Success
    - Swap Oraidex V2 Operation
    - Swap Oraidex V3 Operation
    - Convert Operation
    - Convert Reverse Operation

Expect Error
    - No Native Offer Asset In Contract Balance To Swap
    - No Cw20 Offer Asset In Contract Balance To Swap
    - Unauthorized Caller

 */

// Define test parameters
struct Params {
    caller: String,
    contract_balance: Vec<Coin>,
    swap_operation: SwapOperation,
    expected_message: Option<SubMsg>,
    expected_error: Option<ContractError>,
}

// Test execute_swap
#[test_case(
    Params {
        caller: "swap_contract_address".to_string(),
        contract_balance: vec![Coin::new(100, "os")],
        swap_operation: SwapOperation {
                pool: "pool_1".to_string(),
                denom_in: "orai123".to_string(),
                denom_out: "atom".to_string(),
                interface: None,
            },
        expected_message: Some(SubMsg {
                id: 0,
                msg: WasmMsg::Execute {
                    contract_addr: "orai123".to_string(),
                    msg: to_json_binary(&Cw20ExecuteMsg::Send { 
                        contract: "oraidex_router".to_string(), 
                        amount: Uint128::new(100u128), 
                      msg: to_json_binary(&OraidexRouterExecuteMsg::ExecuteSwapOperations { operations: vec![OraidexSwapOperation::OraiSwap { offer_asset_info: AssetInfo::Token { contract_addr: Addr::unchecked("orai123") }, ask_asset_info:AssetInfo::Token { contract_addr: Addr::unchecked("atom") } } ], minimum_receive: None, to: None })?,
                    })?,
                   
                    funds: vec![],
                }
                .into(),
                gas_limit: None,
                reply_on: Never,
            }),
        expected_error: None,
    };
    "Swap oraidex v2")]
#[test_case(
    Params {
        caller: "swap_contract_address".to_string(),
        contract_balance: vec![Coin::new(100, "os")],
        swap_operation: SwapOperation {
                pool: "orai123-atom-3000000000-10".to_string(),
                denom_in: "orai123".to_string(),
                denom_out: "atom".to_string(),
                interface: None,
            },
        expected_message: Some(SubMsg {
                id: 0,
                msg: WasmMsg::Execute {
                    contract_addr: "orai123".to_string(),
                    msg: to_json_binary(&Cw20ExecuteMsg::Send { 
                        contract: "oraidex_router".to_string(), 
                        amount: Uint128::new(100u128), 
                      msg: to_json_binary(&OraidexRouterExecuteMsg::ExecuteSwapOperations { 
                        operations: vec![
                           OraidexSwapOperation::SwapV3 { pool_key: PoolKey{token_x: "orai123".to_string(), token_y: "atom".to_string(), fee_tier: FeeTier {fee: Percentage(3000000000), tick_spacing: 10}}, x_to_y: true } 
                        ],
                        minimum_receive: None, 
                        to: None })?,
                    })?,
                   
                    funds: vec![],
                }
                .into(),
                gas_limit: None,
                reply_on: Never,
            }),
        expected_error: None,
    };
    "Swap oraidex v3")]
#[test_case(
    Params {
        caller: "swap_contract_address".to_string(),
        contract_balance: vec![Coin::new(100, "os")],
        swap_operation: SwapOperation {
                pool: "convert-orai1converter".to_string(),
                denom_in: "orai123".to_string(),
                denom_out: "orai123_converted".to_string(),
                interface: None,
            },
        expected_message: Some(SubMsg {
                id: 0,
                msg: WasmMsg::Execute {
                    contract_addr: "orai123".to_string(),
                    msg: to_json_binary(&Cw20ExecuteMsg::Send { 
                        contract: "orai1converter".to_string(), 
                        amount: Uint128::new(100u128), 
                      msg: to_json_binary(&ConverterExecuteMsg::Convert {})?,
                    })?,
                   
                    funds: vec![],
                }
                .into(),
                gas_limit: None,
                reply_on: Never,
            }),
        expected_error: None,
    };
    "convert operation")]
#[test_case(
    Params {
        caller: "swap_contract_address".to_string(),
        contract_balance: vec![Coin::new(100, "os")],
        swap_operation: SwapOperation {
                pool: "convert_reverse-orai1converter".to_string(),
                denom_in: "orai123".to_string(),
                denom_out: "orai123_converted".to_string(),
                interface: None,
            },
        expected_message: Some(SubMsg {
                id: 0,
                msg: WasmMsg::Execute {
                    contract_addr: "orai123".to_string(),
                    msg: to_json_binary(&Cw20ExecuteMsg::Send { 
                        contract: "orai1converter".to_string(), 
                        amount: Uint128::new(100u128), 
                      msg: to_json_binary(&ConverterCw20HookMsg::ConvertReverse { from: AssetInfo::Token { contract_addr: Addr::unchecked("orai123_converted") } } )?,
                    })?,
                   
                    funds: vec![],
                }
                .into(),
                gas_limit: None,
                reply_on: Never,
            }),
        expected_error: None,
    };
    "convert reverse operation")]
#[test_case(
    Params {
        caller: "swap_contract_address".to_string(),
        contract_balance: vec![],
        swap_operation: SwapOperation {
                pool: "pool_1".to_string(),
                denom_in: "os".to_string(),
                denom_out: "ua".to_string(),
                interface: None,
            },
        expected_message: None,
        expected_error: Some(ContractError::NoOfferAssetAmount),
    };
    "No Native Offer Asset In Contract Balance To Swap")]
#[test_case(
    Params {
        caller: "swap_contract_address".to_string(),
        contract_balance: vec![],
        swap_operation: SwapOperation {
                pool: "pool_1".to_string(),
                denom_in: "randomcw20".to_string(),
                denom_out: "ua".to_string(),
                interface: None,
            },
        expected_message: None,
        expected_error: Some(ContractError::NoOfferAssetAmount),
    };
    "No Cw20 Offer Asset In Contract Balance To Swap")]
#[test_case(
    Params {
        caller: "random".to_string(),
        contract_balance: vec![
            Coin::new(100, "un"),
        ],
        swap_operation: SwapOperation{
            pool: "".to_string(),
            denom_in: "".to_string(),
            denom_out: "".to_string(),
            interface: None,
        },
        expected_message: None,
        expected_error: Some(ContractError::Unauthorized),
    };
    "Unauthorized Caller - Expect Error")]
fn test_execute_oraidex_pool_swap(params: Params) -> ContractResult<()> {
    // Create mock dependencies
    let mut deps =
        mock_dependencies_with_balances(&[("swap_contract_address", &params.contract_balance)]);

    // Create mock wasm handler to handle the cw20 balance queries
    let wasm_handler = |query: &WasmQuery| -> QuerierResult {
        match query {
            WasmQuery::Smart { contract_addr, .. } => {
                if contract_addr == "orai123" {
                    SystemResult::Ok(
                        ContractResult::Ok(
                            to_json_binary(&BalanceResponse {
                                balance: Uint128::from(100u128),
                            })
                            .unwrap(),
                        )
                        .into(),
                    )
                } else {
                    SystemResult::Ok(
                        ContractResult::Ok(
                            to_json_binary(&BalanceResponse {
                                balance: Uint128::from(0u128),
                            })
                            .unwrap(),
                        )
                        .into(),
                    )
                }
            }
            _ => panic!("Unsupported query: {:?}", query),
        }
    };
    deps.querier.update_wasm(wasm_handler);

    // Create mock env
    let mut env = mock_env();
    env.contract.address = Addr::unchecked("swap_contract_address");

    // Create mock info
    let info = mock_info(&params.caller, &[]);

    // Store the entry point contract address
    ENTRY_POINT_CONTRACT_ADDRESS.save(deps.as_mut().storage, &Addr::unchecked("entry_point"))?;
    ORAIDEX_ROUTER_ADDRESS.save(deps.as_mut().storage, &Addr::unchecked("oraidex_router"))?;

    // Call execute_astroport_pool_swap with the given test parameters
    let res = skip_api_swap_adapter_oraidex::contract::execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::OraidexPoolSwap {
            operation: params.swap_operation,
        },
    );

    // Assert the behavior is correct
    match res {
        Ok(res) => {
            // Assert the test did not expect an error
            assert!(
                params.expected_error.is_none(),
                "expected test to error with {:?}, but it succeeded",
                params.expected_error
            );

            // Assert the messages are correct
            assert_eq!(res.messages[0], params.expected_message.unwrap());
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

    Ok(())
}
