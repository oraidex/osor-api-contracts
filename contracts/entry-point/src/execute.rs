use std::{str::FromStr, vec};

use crate::{
    error::{ContractError, ContractResult},
    reply::{RecoverTempStorage, RECOVER_REPLY_ID},
    state::{
        BLOCKED_CONTRACT_ADDRESSES, IBC_TRANSFER_CONTRACT_ADDRESS, IBC_WASM_CONTRACT_ADDRESS,
        OWNER, PRE_SWAP_OUT_ASSET_AMOUNT, RECOVER_TEMP_STORAGE, SWAP_VENUE_MAP,
    },
};
use cosmwasm_std::{
    from_json, to_json_binary, Addr, Api, BankMsg, Binary, Coin, CosmosMsg, DepsMut, Env,
    MessageInfo, Response, SubMsg, Uint128, WasmMsg,
};
use cw20::{Cw20Coin, Cw20ExecuteMsg, Cw20ReceiveMsg};
use cw_utils::one_coin;
use oraiswap::universal_swap_memo::{
    memo::{PostAction, UserSwap},
    Memo,
};
use skip::{
    asset::{get_current_asset_available, Asset},
    entry_point::{Action, Affiliate, Cw20HookMsg, ExecuteMsg},
    ibc::{ExecuteMsg as IbcTransferExecuteMsg, IbcTransfer},
    ibc_wasm::{ExecuteMsg as IbcWasmTransferExecuteMsg, IbcWasmTransfer},
    swap::{
        validate_swap_operations, ExecuteMsg as SwapExecuteMsg, QueryMsg as SwapQueryMsg,
        SmartSwapExactAssetIn, Swap, SwapExactAssetIn, SwapExactAssetOut, SwapVenue,
    },
};

//////////////////////////
/// RECEIVE ENTRYPOINT ///
//////////////////////////

// Receive is the main entry point for the contract to
// receive cw20 tokens and execute the swap and action message
pub fn receive_cw20(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    cw20_msg: Cw20ReceiveMsg,
) -> ContractResult<Response> {
    let sent_asset = Asset::Cw20(Cw20Coin {
        address: info.sender.to_string(),
        amount: cw20_msg.amount,
    });

    match from_json(&cw20_msg.msg)? {
        Cw20HookMsg::SwapAndActionWithRecover {
            user_swap,
            min_asset,
            timeout_timestamp,
            post_swap_action,
            affiliates,
            recovery_addr,
        } => execute_swap_and_action_with_recover(
            deps,
            env,
            info,
            Some(sent_asset),
            user_swap,
            min_asset,
            timeout_timestamp,
            post_swap_action,
            affiliates,
            recovery_addr,
        ),
        Cw20HookMsg::SwapAndAction {
            user_swap,
            min_asset,
            timeout_timestamp,
            post_swap_action,
            affiliates,
        } => execute_swap_and_action(
            deps,
            env,
            info,
            Some(sent_asset),
            user_swap,
            min_asset,
            timeout_timestamp,
            post_swap_action,
            affiliates,
        ),
        Cw20HookMsg::UniversalSwap { memo } => {
            execute_universal_swap(deps, env, info, Some(sent_asset), memo)
        }
    }
}

///////////////////////////
/// EXECUTE ENTRYPOINTS ///
///////////////////////////

// withdraw stuck coin and transfer back to user
// only owner can execute
pub fn execute_withdraw_asset(
    deps: DepsMut,
    info: MessageInfo,
    coin: Asset,
    receiver: Option<Addr>,
) -> Result<Response, ContractError> {
    OWNER.assert_admin(deps.as_ref(), &info.sender)?;
    let receiver = receiver.unwrap_or(info.sender);

    let msg = coin.transfer(receiver.as_str());

    Ok(Response::new()
        .add_attributes(vec![("action", "withdraw_asset")])
        .add_message(msg))
}

// update config
pub fn execute_update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<Addr>,
    swap_venues: Option<Vec<SwapVenue>>,
    ibc_transfer_contract_address: Option<String>,
    ibc_wasm_contract_address: Option<String>,
) -> ContractResult<Response> {
    OWNER.assert_admin(deps.as_ref(), &info.sender)?;

    let mut response: Response = Response::new().add_attribute("action", "update_config");

    // Iterate through the swap venues provided and create a map of venue names to swap adapter contract addresses
    if let Some(swap_venues) = swap_venues {
        for swap_venue in swap_venues.iter() {
            // Validate the swap contract address
            let checked_swap_contract_address = deps
                .api
                .addr_validate(&swap_venue.adapter_contract_address)?;

            // Prevent duplicate swap venues by erroring if the venue name is already stored
            if SWAP_VENUE_MAP.has(deps.storage, &swap_venue.name) {
                return Err(ContractError::DuplicateSwapVenueName);
            }

            // Store the swap venue name and contract address inside the swap venue map
            SWAP_VENUE_MAP.save(
                deps.storage,
                &swap_venue.name,
                &checked_swap_contract_address,
            )?;

            // Insert the swap contract address into the blocked contract addresses map
            BLOCKED_CONTRACT_ADDRESSES.save(deps.storage, &checked_swap_contract_address, &())?;

            // Add the swap venue and contract address to the response
            response = response
                .add_attribute("action", "add_swap_venue")
                .add_attribute("name", &swap_venue.name)
                .add_attribute("contract_address", &checked_swap_contract_address);
        }
    }

    if let Some(ibc_transfer_contract_address) = ibc_transfer_contract_address {
        // Validate ibc transfer adapter contract addresses
        let checked_ibc_transfer_contract_address =
            deps.api.addr_validate(&ibc_transfer_contract_address)?;

        // Store the ibc transfer adapter contract address
        IBC_TRANSFER_CONTRACT_ADDRESS.save(deps.storage, &checked_ibc_transfer_contract_address)?;

        // Insert the ibc transfer adapter contract address into the blocked contract addresses map
        BLOCKED_CONTRACT_ADDRESSES.save(
            deps.storage,
            &checked_ibc_transfer_contract_address,
            &(),
        )?;

        // Add the ibc transfer adapter contract address to the response
        response = response
            .add_attribute("action", "add_ibc_transfer_adapter")
            .add_attribute("contract_address", &checked_ibc_transfer_contract_address);
    }

    if let Some(ibc_wasm_contract_address) = ibc_wasm_contract_address {
        // Validate ibc transfer adapter contract addresses
        let checked_ibc_wasm_contract_address =
            deps.api.addr_validate(&ibc_wasm_contract_address)?;

        // Store the ibc transfer adapter contract address
        IBC_WASM_CONTRACT_ADDRESS.save(deps.storage, &checked_ibc_wasm_contract_address)?;

        // Insert the ibc transfer adapter contract address into the blocked contract addresses map
        BLOCKED_CONTRACT_ADDRESSES.save(deps.storage, &checked_ibc_wasm_contract_address, &())?;

        // Add the ibc transfer adapter contract address to the response
        response = response
            .add_attribute("action", "add_ibc_wasm_transfer_adapter")
            .add_attribute("contract_address", &checked_ibc_wasm_contract_address);
    }

    if let Some(owner) = owner {
        response = response.add_attribute("new_owner", owner.as_str());
        OWNER.set(deps, Some(owner))?;
    }

    Ok(response)
}

pub fn execute_universal_swap(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    sent_asset: Option<Asset>,
    memo: String,
) -> ContractResult<Response> {
    // Create a response object to return
    let mut response: Response = Response::new().add_attribute("action", "execute_universal_swap");

    // Validate and unwrap the sent asset
    let sent_asset = match sent_asset {
        Some(sent_asset) => {
            sent_asset.validate(&deps, &env, &info)?;
            sent_asset
        }
        None => one_coin(&info)?.into(),
    };
    let memo_data = Memo::decode_memo(Binary::from_base64(&memo)?)?;
    // let user_swap = memo_data.user_swap.unwrap_or_default();
    let mut cosmos_msgs: Vec<CosmosMsg> = vec![];

    // call swap and action with recover
    if let (Some(post_action), Some(user_swap)) = (
        memo_data.post_swap_action.clone(),
        memo_data.user_swap.clone(),
    ) {
        convert_user_swap_with_action_to_cosmos_msgs(
            deps.api,
            env.contract.address.as_str(),
            &mut cosmos_msgs,
            user_swap,
            post_action,
            &memo_data.minimum_receive,
            sent_asset,
            memo_data.timeout_timestamp,
            deps.api.addr_validate(&memo_data.recovery_addr)?,
        )?;
    } else if let Some(post_action) = memo_data.post_swap_action {
        let pre_swap_out_asset_amount =
            get_current_asset_available(&deps, &env, sent_asset.denom())?
                .amount()
                .checked_sub(sent_asset.amount())?;

        PRE_SWAP_OUT_ASSET_AMOUNT.save(deps.storage, &pre_swap_out_asset_amount)?;

        let min_asset = Asset::new(
            deps.api,
            &sent_asset.denom(),
            Uint128::from_str(&memo_data.minimum_receive)?,
        );
        let post_swap_action_msg = WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::PostSwapAction {
                min_asset,
                timeout_timestamp: memo_data.timeout_timestamp,
                post_swap_action: Action::try_from(post_action, memo_data.timeout_timestamp)?,
                exact_out: false,
            })?,
            funds: vec![],
        };
        cosmos_msgs.push(post_swap_action_msg.into());
    } else {
        // fallback case, we send the sent tokens back to recovery addr
        cosmos_msgs.push(sent_asset.transfer(&memo_data.recovery_addr))
    }

    response = response.add_messages(cosmos_msgs);
    Ok(response)

    // handle post swap
}

pub fn convert_user_swap_with_action_to_cosmos_msgs(
    api: &dyn Api,
    env_contract_address: &str,
    cosmos_msgs: &mut Vec<CosmosMsg>,
    user_swap: UserSwap,
    post_swap_action: PostAction,
    minimum_receive: &str,
    sent_asset: Asset,
    timeout_timestamp: u64,
    recovery_addr: Addr,
) -> Result<(), ContractError> {
    // if not a smart route -> transform into adapter messages
    if let Some(swap_exact) = user_swap.swap_exact_asset_in {
        let swap_exact_in = SwapExactAssetIn::from(&user_swap.swap_venue_name, &swap_exact);
        let min_asset = swap_exact_in.get_min_asset(api, minimum_receive)?;

        let msg = to_json_binary(&ExecuteMsg::SwapAndActionWithRecover {
            sent_asset: Some(sent_asset.clone()),
            user_swap: Swap::SwapExactAssetIn(SwapExactAssetIn::from(
                &user_swap.swap_venue_name,
                &swap_exact,
            )),
            min_asset,
            timeout_timestamp,
            post_swap_action: Action::try_from(post_swap_action, timeout_timestamp)?,
            affiliates: vec![],
            recovery_addr,
        })?;
        let funds = match sent_asset {
            Asset::Native(coin) => vec![coin],
            Asset::Cw20(_) => vec![],
        };
        cosmos_msgs.push(
            WasmMsg::Execute {
                contract_addr: env_contract_address.to_string(),
                msg,
                funds,
            }
            .into(),
        );
        return Ok(());
    }
    let smart_swap_exact = user_swap.smart_swap_exact_asset_in.unwrap_or_default();
    let smart_swap_exact_in = SmartSwapExactAssetIn::from(
        api,
        sent_asset.denom(),
        &user_swap.swap_venue_name,
        &smart_swap_exact,
    );
    let min_asset = smart_swap_exact_in.get_min_asset(api, minimum_receive)?;
    let msg = to_json_binary(&ExecuteMsg::SwapAndActionWithRecover {
        sent_asset: Some(sent_asset.clone()),
        user_swap: Swap::SmartSwapExactAssetIn(smart_swap_exact_in),
        min_asset,
        timeout_timestamp,
        post_swap_action: Action::try_from(post_swap_action, timeout_timestamp)?,
        affiliates: vec![],
        recovery_addr,
    })?;
    let funds = match sent_asset {
        Asset::Native(coin) => vec![coin],
        Asset::Cw20(_) => vec![],
    };
    // dont use into_wasm_msg because we are calling a self-execute msg, there's no need to send from cw20 to self with the same amount.
    cosmos_msgs.push(
        WasmMsg::Execute {
            contract_addr: env_contract_address.to_string(),
            msg,
            funds,
        }
        .into(),
    );
    Ok(())
}

// Main entry point for the contract
// Dispatches the swap and post swap action
#[allow(clippy::too_many_arguments)]
pub fn execute_swap_and_action(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    sent_asset: Option<Asset>,
    mut user_swap: Swap,
    min_asset: Asset,
    timeout_timestamp: u64,
    post_swap_action: Action,
    affiliates: Vec<Affiliate>,
) -> ContractResult<Response> {
    // Create a response object to return
    let mut response: Response = Response::new().add_attribute("action", "execute_swap_and_action");

    // Validate and unwrap the sent asset
    let sent_asset = match sent_asset {
        Some(sent_asset) => {
            sent_asset.validate(&deps, &env, &info)?;
            sent_asset
        }
        None => one_coin(&info)?.into(),
    };

    // Error if the current block time is greater than the timeout timestamp
    if env.block.time.nanos() > timeout_timestamp {
        return Err(ContractError::Timeout);
    }

    // Save the current out asset amount to storage as the pre swap out asset amount
    let pre_swap_out_asset_amount =
        get_current_asset_available(&deps, &env, min_asset.denom())?.amount();
    PRE_SWAP_OUT_ASSET_AMOUNT.save(deps.storage, &pre_swap_out_asset_amount)?;

    // Already validated at entrypoints (both direct and cw20_receive)
    let mut remaining_asset = sent_asset;

    // If the post swap action is an IBC transfer, then handle the ibc fees
    // by either creating a fee swap message or deducting the ibc fees from
    // the remaining asset received amount.
    if let Action::IbcTransfer { ibc_info, fee_swap } = &post_swap_action {
        let ibc_fee_coin = ibc_info
            .fee
            .as_ref()
            .map(|fee| fee.one_coin())
            .transpose()?;

        if let Some(fee_swap) = fee_swap {
            let ibc_fee_coin = ibc_fee_coin
                .clone()
                .ok_or(ContractError::FeeSwapWithoutIbcFees)?;

            // NOTE: this call mutates remaining_asset by deducting ibc_fee_coin's amount from it
            let fee_swap_msg = verify_and_create_fee_swap_msg(
                &deps,
                fee_swap,
                &mut remaining_asset,
                &ibc_fee_coin,
            )?;

            // Add the fee swap message to the response
            response = response
                .add_message(fee_swap_msg)
                .add_attribute("action", "dispatch_fee_swap");
        } else if let Some(ibc_fee_coin) = &ibc_fee_coin {
            if remaining_asset.denom() != ibc_fee_coin.denom {
                return Err(ContractError::IBCFeeDenomDiffersFromAssetReceived);
            }

            // Deduct the ibc_fee_coin amount from the remaining asset amount
            remaining_asset.sub(ibc_fee_coin.amount)?;
        }

        // Dispatch the ibc fee bank send to the ibc transfer adapter contract if needed
        if let Some(ibc_fee_coin) = ibc_fee_coin {
            // Get the ibc transfer adapter contract address
            let ibc_transfer_contract_address = IBC_TRANSFER_CONTRACT_ADDRESS.load(deps.storage)?;

            // Create the ibc fee bank send message
            let ibc_fee_msg = BankMsg::Send {
                to_address: ibc_transfer_contract_address.to_string(),
                amount: vec![ibc_fee_coin],
            };

            // Add the ibc fee message to the response
            response = response
                .add_message(ibc_fee_msg)
                .add_attribute("action", "dispatch_ibc_fee_bank_send");
        }
    }

    // Set a boolean to determine if the user swap is exact out or not
    let exact_out = match &user_swap {
        Swap::SwapExactAssetIn(_) => false,
        Swap::SwapExactAssetOut(_) => true,
        Swap::SmartSwapExactAssetIn(_) => false,
    };

    if let Swap::SmartSwapExactAssetIn(smart_swap) = &mut user_swap {
        if smart_swap.routes.is_empty() {
            return Err(ContractError::Skip(skip::error::SkipError::RoutesEmpty));
        }

        match smart_swap.amount().cmp(&remaining_asset.amount()) {
            std::cmp::Ordering::Equal => {}
            std::cmp::Ordering::Less => {
                let diff = remaining_asset.amount().checked_sub(smart_swap.amount())?;

                // If the total swap in amount is less than remaining asset,
                // adjust the routes to match the remaining asset amount
                let largest_route_idx = smart_swap.largest_route_index()?;

                smart_swap.routes[largest_route_idx].offer_asset.add(diff)?;
            }
            std::cmp::Ordering::Greater => {
                let diff = smart_swap.amount().checked_sub(remaining_asset.amount())?;

                // If the total swap in amount is greater than remaining asset,
                // adjust the routes to match the remaining asset amount
                let largest_route_idx = smart_swap.largest_route_index()?;

                smart_swap.routes[largest_route_idx].offer_asset.sub(diff)?;
            }
        }
    }

    let user_swap_msg = WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_json_binary(&ExecuteMsg::UserSwap {
            swap: user_swap,
            min_asset: min_asset.clone(),
            remaining_asset,
            affiliates,
        })?,
        funds: vec![],
    };

    // Add the user swap message to the response
    response = response
        .add_message(user_swap_msg)
        .add_attribute("action", "dispatch_user_swap");

    // Create the post swap action message
    let post_swap_action_msg = WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_json_binary(&ExecuteMsg::PostSwapAction {
            min_asset,
            timeout_timestamp,
            post_swap_action,
            exact_out,
        })?,
        funds: vec![],
    };

    // Add the post swap action message to the response and return the response
    Ok(response
        .add_message(post_swap_action_msg)
        .add_attribute("action", "dispatch_post_swap_action"))
}

// Entrypoint that catches all errors in SwapAndAction and recovers
// the original funds sent to the contract to a recover address.
#[allow(clippy::too_many_arguments)]
pub fn execute_swap_and_action_with_recover(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    sent_asset: Option<Asset>,
    user_swap: Swap,
    min_asset: Asset,
    timeout_timestamp: u64,
    post_swap_action: Action,
    affiliates: Vec<Affiliate>,
    recovery_addr: Addr,
) -> ContractResult<Response> {
    let mut assets: Vec<Asset> = info.funds.iter().cloned().map(Asset::Native).collect();

    if let Some(asset) = &sent_asset {
        if let Asset::Cw20(_) = asset {
            assets.push(asset.clone());
        }
    }

    // Store all parameters into a temporary storage.
    RECOVER_TEMP_STORAGE.save(
        deps.storage,
        &RecoverTempStorage {
            assets,
            recovery_addr,
        },
    )?;

    // Then call ExecuteMsg::SwapAndAction using a SubMsg.
    let sub_msg = SubMsg::reply_always(
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::SwapAndAction {
                sent_asset,
                user_swap,
                min_asset,
                timeout_timestamp,
                post_swap_action,
                affiliates,
            })?,
            funds: info.funds,
        }),
        RECOVER_REPLY_ID,
    );
    Ok(Response::new().add_submessage(sub_msg))
}

// Dispatches the user swap and refund/affiliate fee bank sends if needed
pub fn execute_user_swap(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    swap: Swap,
    mut min_asset: Asset,
    mut remaining_asset: Asset,
    affiliates: Vec<Affiliate>,
) -> ContractResult<Response> {
    // Enforce the caller is the contract itself
    if info.sender != env.contract.address {
        return Err(ContractError::Unauthorized);
    }

    // Create a response object to return
    let mut response: Response = Response::new()
        .add_attribute("action", "execute_user_swap")
        .add_attribute("denom_in", remaining_asset.denom())
        .add_attribute("denom_out", min_asset.denom());

    // Create affiliate response and total affiliate fee amount
    let mut affiliate_response: Response = Response::new();
    let mut total_affiliate_fee_amount: Uint128 = Uint128::zero();

    // If affiliates exist, create the affiliate fee messages and attributes and
    // add them to the affiliate response, updating the total affiliate fee amount
    for affiliate in affiliates.iter() {
        // Verify, calculate, and get the affiliate fee amount
        let affiliate_fee_amount =
            verify_and_calculate_affiliate_fee_amount(&deps, &min_asset, affiliate)?;

        if affiliate_fee_amount > Uint128::zero() {
            // Add the affiliate fee amount to the total affiliate fee amount
            total_affiliate_fee_amount =
                total_affiliate_fee_amount.checked_add(affiliate_fee_amount)?;

            // Create the affiliate_fee_asset
            let affiliate_fee_asset = Asset::new(deps.api, min_asset.denom(), affiliate_fee_amount);

            // Create the affiliate fee message
            let affiliate_fee_msg = affiliate_fee_asset.transfer(&affiliate.address);

            // Add the affiliate fee message and attributes to the response
            affiliate_response = affiliate_response
                .add_message(affiliate_fee_msg)
                .add_attribute("action", "dispatch_affiliate_fee_bank_send")
                .add_attribute("address", &affiliate.address)
                .add_attribute("amount", affiliate_fee_amount);
        }
    }

    // Create the user swap message
    match swap {
        Swap::SwapExactAssetIn(swap) => {
            // Validate swap operations
            validate_swap_operations(&swap.operations, remaining_asset.denom(), min_asset.denom())?;

            // Get swap adapter contract address from venue name
            let user_swap_adapter_contract_address =
                SWAP_VENUE_MAP.load(deps.storage, &swap.swap_venue_name)?;

            // Create the user swap message args
            let user_swap_msg_args: SwapExecuteMsg = swap.into();

            // Create the user swap message
            let user_swap_msg = remaining_asset.into_wasm_msg(
                user_swap_adapter_contract_address.to_string(),
                to_json_binary(&user_swap_msg_args)?,
            )?;

            response = response
                .add_message(user_swap_msg)
                .add_attribute("action", "dispatch_user_swap_exact_asset_in");
        }
        Swap::SwapExactAssetOut(swap) => {
            // Validate swap operations
            validate_swap_operations(&swap.operations, remaining_asset.denom(), min_asset.denom())?;

            // Get swap adapter contract address from venue name
            let user_swap_adapter_contract_address =
                SWAP_VENUE_MAP.load(deps.storage, &swap.swap_venue_name)?;

            // Calculate the swap asset out by adding the min asset amount to the total affiliate fee amount
            min_asset.add(total_affiliate_fee_amount)?;

            // Query the swap adapter to get the asset in needed to obtain the min asset plus affiliates
            let user_swap_asset_in = query_swap_asset_in(
                &deps,
                &user_swap_adapter_contract_address,
                &swap,
                &min_asset,
            )?;

            // Verify the user swap in denom is the same as the denom received from the message to the contract
            if user_swap_asset_in.denom() != remaining_asset.denom() {
                return Err(ContractError::UserSwapAssetInDenomMismatch);
            }

            // Calculate refund amount to send back to the user
            remaining_asset.sub(user_swap_asset_in.amount())?;

            // If refund amount gt zero, then create the refund message and add it to the response
            if remaining_asset.amount() > Uint128::zero() {
                // Get the refund address from the swap
                let to_address = swap
                    .refund_address
                    .clone()
                    .ok_or(ContractError::NoRefundAddress)?;

                // Validate the refund address
                deps.api.addr_validate(&to_address)?;

                // Get the refund amount
                let refund_amount = remaining_asset.amount();

                // Create the refund message
                let refund_msg = remaining_asset.transfer(&to_address);

                // Add the refund message and attributes to the response
                response = response
                    .add_message(refund_msg)
                    .add_attribute("action", "dispatch_refund")
                    .add_attribute("address", &to_address)
                    .add_attribute("amount", refund_amount);
            }

            // Create the user swap message args
            let user_swap_msg_args: SwapExecuteMsg = swap.into();

            // Create the user swap message
            let user_swap_msg = user_swap_asset_in.into_wasm_msg(
                user_swap_adapter_contract_address.to_string(),
                to_json_binary(&user_swap_msg_args)?,
            )?;

            response = response
                .add_message(user_swap_msg)
                .add_attribute("action", "dispatch_user_swap_exact_asset_out");
        }
        Swap::SmartSwapExactAssetIn(swap) => {
            for route in swap.routes {
                // Validate swap operations
                validate_swap_operations(
                    &route.operations,
                    remaining_asset.denom(),
                    min_asset.denom(),
                )?;

                // Get swap adapter contract address from venue name
                let user_swap_adapter_contract_address =
                    SWAP_VENUE_MAP.load(deps.storage, &swap.swap_venue_name)?;

                // Create the user swap message args
                let user_swap_msg_args = SwapExecuteMsg::Swap {
                    operations: route.operations,
                };

                // Create the user swap message
                let user_swap_msg = route.offer_asset.into_wasm_msg(
                    user_swap_adapter_contract_address.to_string(),
                    to_json_binary(&user_swap_msg_args)?,
                )?;

                response = response
                    .add_message(user_swap_msg)
                    .add_attribute("action", "dispatch_user_swap_exact_asset_in");
            }
        }
    }

    // Add the affiliate messages and attributes to the response and return the response
    // Having the affiliate messages after the swap is purposeful, so that the affiliate
    // bank sends are valid and the contract has funds to send to the affiliates.
    Ok(response
        .add_submessages(affiliate_response.messages)
        .add_attributes(affiliate_response.attributes))
}

// Dispatches the post swap action
// Can only be called by the contract itself
pub fn execute_post_swap_action(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    min_asset: Asset,
    timeout_timestamp: u64,
    post_swap_action: Action,
    exact_out: bool,
) -> ContractResult<Response> {
    // Enforce the caller is the contract itself
    if info.sender != env.contract.address {
        return Err(ContractError::Unauthorized);
    }

    // Create a response object to return
    let mut response: Response =
        Response::new().add_attribute("action", "execute_post_swap_action");

    // Get the pre swap out asset amount from storage
    let pre_swap_out_asset_amount = PRE_SWAP_OUT_ASSET_AMOUNT.load(deps.storage)?;

    // Get contract balance of min out asset post swap
    // for fee deduction and transfer out amount enforcement
    let post_swap_out_asset = get_current_asset_available(&deps, &env, min_asset.denom())?;

    // Set the transfer out asset to the post swap out asset amount minus the pre swap out asset amount
    // Since we only want to transfer out the amount received from the swap
    let transfer_out_asset = Asset::new(
        deps.api,
        min_asset.denom(),
        post_swap_out_asset
            .amount()
            .checked_sub(pre_swap_out_asset_amount)?,
    );

    // Error if the contract balance is less than the min asset out amount
    if transfer_out_asset.amount() < min_asset.amount() {
        return Err(ContractError::ReceivedLessAssetFromSwapsThanMinAsset);
    }

    // Set the transfer out asset to the min asset if exact out is true
    let transfer_out_asset = if exact_out {
        min_asset
    } else {
        transfer_out_asset
    };

    response = response
        .add_attribute(
            "post_swap_action_amount_out",
            transfer_out_asset.amount().to_string(),
        )
        .add_attribute("post_swap_action_denom_out", transfer_out_asset.denom());

    match post_swap_action {
        Action::Transfer { to_address } => {
            // Error if the destination address is not a valid address on the current chain
            deps.api.addr_validate(&to_address)?;

            // Create the transfer message
            let transfer_msg = transfer_out_asset.transfer(&to_address);

            // Add the transfer message to the response
            response = response
                .add_message(transfer_msg)
                .add_attribute("action", "dispatch_post_swap_transfer");
        }
        Action::IbcTransfer { ibc_info, .. } => {
            // Validates recover address, errors if invalid
            deps.api.addr_validate(&ibc_info.recover_address)?;

            let transfer_out_coin = match transfer_out_asset {
                Asset::Native(coin) => coin,
                _ => return Err(ContractError::NonNativeIbcTransfer),
            };

            // Create the IBC transfer message
            let ibc_transfer_msg: IbcTransferExecuteMsg = IbcTransfer {
                info: ibc_info,
                coin: transfer_out_coin.clone(),
                timeout_timestamp,
            }
            .into();

            // Get the IBC transfer adapter contract address
            let ibc_transfer_contract_address = IBC_TRANSFER_CONTRACT_ADDRESS.load(deps.storage)?;

            // Send the IBC transfer by calling the IBC transfer contract
            let ibc_transfer_msg = WasmMsg::Execute {
                contract_addr: ibc_transfer_contract_address.to_string(),
                msg: to_json_binary(&ibc_transfer_msg)?,
                funds: vec![transfer_out_coin],
            };

            // Add the IBC transfer message to the response
            response = response
                .add_message(ibc_transfer_msg)
                .add_attribute("action", "dispatch_post_swap_ibc_transfer");
        }
        Action::ContractCall {
            contract_address,
            msg,
        } => {
            // Verify the contract address is valid, error if invalid
            let checked_contract_address = deps.api.addr_validate(&contract_address)?;

            // Error if the contract address is in the blocked contract addresses map
            if BLOCKED_CONTRACT_ADDRESSES.has(deps.storage, &checked_contract_address) {
                return Err(ContractError::ContractCallAddressBlocked);
            }

            // Create the contract call message
            let contract_call_msg = transfer_out_asset.into_wasm_msg(contract_address, msg)?;

            // Add the contract call message to the response
            response = response
                .add_message(contract_call_msg)
                .add_attribute("action", "dispatch_post_swap_contract_call");
        }
        Action::IbcWasmTransfer { ibc_wasm_info, .. } => {
            // Create the IBC transfer message
            let ibc_transfer_msg: IbcWasmTransferExecuteMsg = IbcWasmTransfer {
                info: ibc_wasm_info,
                coin: transfer_out_asset.clone(),
            }
            .into();

            // Get the IBC transfer adapter contract address
            let ibc_wasm_transfer_contract_address =
                IBC_WASM_CONTRACT_ADDRESS.load(deps.storage)?;

            // Send the IBC transfer by calling the IBC transfer contract
            let ibc_wasm_transfer_msg = match &transfer_out_asset {
                Asset::Native(coin) => WasmMsg::Execute {
                    contract_addr: ibc_wasm_transfer_contract_address.to_string(),
                    msg: to_json_binary(&ibc_transfer_msg)?,
                    funds: vec![coin.clone()],
                },
                Asset::Cw20(coin) => WasmMsg::Execute {
                    contract_addr: coin.clone().address,
                    msg: to_json_binary(&Cw20ExecuteMsg::Send {
                        contract: ibc_wasm_transfer_contract_address.to_string(),
                        amount: coin.amount,
                        msg: to_json_binary(&ibc_transfer_msg)?,
                    })?,
                    funds: vec![],
                },
            };

            // Add the IBC transfer message to the response
            response = response
                .add_message(ibc_wasm_transfer_msg)
                .add_attribute("action", "dispatch_post_swap_ibc_wasm_transfer");
        }
    };

    Ok(response)
}

////////////////////////
/// HELPER FUNCTIONS ///
////////////////////////

// SWAP MESSAGE HELPER FUNCTIONS

// Creates the fee swap message and returns it
// Also deducts the fee swap in amount from the mutable remaining asset
fn verify_and_create_fee_swap_msg(
    deps: &DepsMut,
    fee_swap: &SwapExactAssetOut,
    remaining_asset: &mut Asset,
    ibc_fee_coin: &Coin,
) -> ContractResult<WasmMsg> {
    // Validate swap operations
    validate_swap_operations(
        &fee_swap.operations,
        remaining_asset.denom(),
        &ibc_fee_coin.denom,
    )?;

    // Get swap adapter contract address from venue name
    let fee_swap_adapter_contract_address =
        SWAP_VENUE_MAP.load(deps.storage, &fee_swap.swap_venue_name)?;

    // Query the swap adapter to get the asset in needed for the fee swap
    let fee_swap_asset_in = query_swap_asset_in(
        deps,
        &fee_swap_adapter_contract_address,
        fee_swap,
        &ibc_fee_coin.clone().into(),
    )?;

    // Verify the fee swap in denom is the same as the denom received from the message to the contract
    if fee_swap_asset_in.denom() != remaining_asset.denom() {
        return Err(ContractError::FeeSwapAssetInDenomMismatch);
    }

    // Deduct the fee swap in amount from the remaining asset amount
    // Error if swap requires more than the remaining asset amount
    remaining_asset.sub(fee_swap_asset_in.amount())?;

    // Create the fee swap message args
    let fee_swap_msg_args: SwapExecuteMsg = fee_swap.clone().into();

    // Create the fee swap message
    let fee_swap_msg = fee_swap_asset_in.into_wasm_msg(
        fee_swap_adapter_contract_address.to_string(),
        to_json_binary(&fee_swap_msg_args)?,
    )?;

    Ok(fee_swap_msg)
}

// AFFILIATE FEE HELPER FUNCTIONS

// Verifies the affiliate address is valid, if so then
// returns the calculated affiliate fee amount.
fn verify_and_calculate_affiliate_fee_amount(
    deps: &DepsMut,
    min_asset: &Asset,
    affiliate: &Affiliate,
) -> ContractResult<Uint128> {
    // Verify the affiliate address is valid
    deps.api.addr_validate(&affiliate.address)?;

    // Get the affiliate fee amount by multiplying the min_asset
    // amount by the affiliate basis points fee divided by 10000
    let affiliate_fee_amount = min_asset
        .amount()
        .multiply_ratio(affiliate.basis_points_fee, Uint128::new(10000));

    Ok(affiliate_fee_amount)
}

// QUERY HELPER FUNCTIONS

// Unexposed query helper function that queries the swap adapter contract to get the
// asset in needed for a given swap. Verifies the swap's in denom is the same as the
// swap asset denom from the message. Returns the swap asset in.
fn query_swap_asset_in(
    deps: &DepsMut,
    swap_adapter_contract_address: &Addr,
    swap: &SwapExactAssetOut,
    swap_asset_out: &Asset,
) -> ContractResult<Asset> {
    // Query the swap adapter to get the asset in needed for the fee swap
    let fee_swap_asset_in: Asset = deps.querier.query_wasm_smart(
        swap_adapter_contract_address,
        &SwapQueryMsg::SimulateSwapExactAssetOut {
            asset_out: swap_asset_out.clone(),
            swap_operations: swap.operations.clone(),
        },
    )?;

    Ok(fee_swap_asset_in)
}
