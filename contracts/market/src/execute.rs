use cosmwasm_std::{
    from_binary, to_binary, BankMsg, CosmosMsg, DepsMut, Env, MessageInfo, Reply, Response,
    StdError, StdResult, SubMsg, SubMsgResponse, WasmMsg,
};
use cw2::set_contract_version;

use crate::{
    msg::{
        Cw721ReceiveMsg, ExecuteMsg, ExecuteMsgCw721, InstantiateMsg, ReceiveLazyNftMsg, SaleData,
        TokenMsg,
    },
    state::{Contract, LazyNft, Sale, TEMP, Temp},
    ContractError,
};

const CONTRACT_NAME: &str = "contract";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");
const MINT_RESPONSE_ID: u64 = 1;

impl<'a> Contract<'a> {
    pub fn instantiate(
        &self,
        _deps: DepsMut,
        _env: Env,
        _info: MessageInfo,
        _msg: InstantiateMsg,
    ) -> StdResult<Response> {
        set_contract_version(_deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

        let owner = _info.sender;
        self.owner.save(_deps.storage, &owner)?;

        Ok(Response::default())
    }
}

impl<'a> Contract<'a> {
    pub fn execute(
        &self,
        _deps: DepsMut,
        _env: Env,
        _info: MessageInfo,
        _msg: ExecuteMsg,
    ) -> Result<Response, ContractError> {
        match _msg {
            ExecuteMsg::ReceiveNft(msg) => self.receive_nft(_deps, _env, _info, msg),
            ExecuteMsg::ReceiveLazyNft(msg) => self.receive_lazy_nft(_deps, _env, _info, msg),
            ExecuteMsg::RemoveSale { token_id } => self.remove_sale(_deps, _env, _info, token_id),
            ExecuteMsg::Purchase { token_id } => self.purchase(_deps, _env, _info, token_id),
            ExecuteMsg::PurchaseLazy { token_id } => {
                self.purchase_lazy(_deps, _env, _info, token_id)
            }
        }
    }

    pub fn receive_nft(
        &self,
        _deps: DepsMut,
        _env: Env,
        _info: MessageInfo,
        _msg: Cw721ReceiveMsg,
    ) -> Result<Response, ContractError> {
        let data: SaleData = from_binary(&_msg.msg)?;

        let sale = Sale {
            contract: _info.sender.clone(),
            token_id: _msg.token_id.clone(),
            owner: _deps.api.addr_validate(&_msg.sender)?,
            price: data.price.clone(),
        };

        self.sales
            .update(_deps.storage, &_msg.token_id, |old| match old {
                Some(_) => Err(ContractError::AlreadyExists {}),
                None => Ok(sale),
            })?;

        Ok(Response::new()
            .add_attribute("action", "receive")
            .add_attribute("token_id", _msg.token_id)
            .add_attribute("owner", _msg.sender)
            .add_attribute("buy_token", data.price.denom)
            .add_attribute("buy_price", data.price.amount))
    }

    pub fn receive_lazy_nft(
        &self,
        _deps: DepsMut,
        _env: Env,
        _info: MessageInfo,
        _msg: ReceiveLazyNftMsg,
    ) -> Result<Response, ContractError> {
        let lazy_nft = LazyNft {
            contract: _deps.api.addr_validate(&_msg.contract)?,
            token_id: _msg.token_id.clone(),
        };

        self.lazy_sales
            .update(_deps.storage, &_msg.token_id, |old| match old {
                Some(_) => Err(ContractError::AlreadyExists {}),
                None => Ok(lazy_nft),
            })?;

        Ok(Response::default())
    }

    pub fn remove_sale(
        &self,
        _deps: DepsMut,
        _env: Env,
        _info: MessageInfo,
        token_id: String,
    ) -> Result<Response, ContractError> {
        let sale = self.sales.load(_deps.storage, &token_id)?;

        if _info.sender != sale.owner {
            return Result::Err(ContractError::Unauthorized {});
        }

        self.sales.remove(_deps.storage, &token_id)?;

        Ok(Response::new()
            .add_attribute("action", "remove_sale")
            .add_attribute("token_id", sale.token_id))
    }

    pub fn purchase(
        &self,
        _deps: DepsMut,
        _env: Env,
        _info: MessageInfo,
        token_id: String,
    ) -> Result<Response, ContractError> {
        let sale = self.sales.load(_deps.storage, &token_id)?;

        let coin = match _info
            .funds
            .into_iter()
            .find(|coin| coin.denom == sale.price.denom)
        {
            Some(coin) => coin,
            None => return Err(ContractError::InvalidDeposit {}),
        };

        if coin.amount != sale.price.amount {
            return Err(ContractError::InvalidDeposit {});
        }

        self.sales.remove(_deps.storage, &token_id)?;

        let funds_transfer: CosmosMsg = BankMsg::Send {
            to_address: sale.owner.into_string(),
            amount: vec![coin.clone()],
        }
        .into();

        let transfer = ExecuteMsgCw721::TransferNft {
            to: _info.sender.clone().into_string(),
            token_id: token_id.clone(),
        };
        let transfer_msg: CosmosMsg = WasmMsg::Execute {
            contract_addr: sale.contract.into(),
            msg: to_binary(&transfer)?,
            funds: vec![],
        }
        .into();

        Ok(Response::new()
            .add_messages(vec![funds_transfer, transfer_msg])
            .add_attribute("action", "purchase")
            .add_attribute("buyer", _info.sender.into_string())
            .add_attribute("token_id", token_id)
            .add_attribute("buy_token", coin.denom)
            .add_attribute("buy_price", coin.amount))
    }

    pub fn purchase_lazy(
        &self,
        _deps: DepsMut,
        _env: Env,
        _info: MessageInfo,
        token_id: String,
    ) -> Result<Response, ContractError> {
        let lazy_nft = self.lazy_sales.load(_deps.storage, &token_id)?;

        let mint = ExecuteMsgCw721::Mint {
            token: TokenMsg {
                owner: _info.sender.into_string(),
                token_id: token_id.clone(),
                token_uri: None,
            },
        };
        let mint_msg = SubMsg::reply_on_success(
            WasmMsg::Execute {
                contract_addr: lazy_nft.contract.into_string(),
                msg: to_binary(&mint)?,
                funds: vec![],
            },
            MINT_RESPONSE_ID,
        );

        let temp = Temp {
            funds: _info.funds.into_iter().find(|coin| coin.denom == "umlg").unwrap()
        };
        TEMP.save(_deps.storage, &temp)?;

        Ok(Response::new().add_submessage(mint_msg))
    }
}

impl<'a> Contract<'a> {
    pub fn reply(&self, _deps: DepsMut, _env: Env, _reply: Reply) -> StdResult<Response> {
        match _reply.id {
            MINT_RESPONSE_ID => self.mint_reply(_deps, _env, _reply),
            _ => Err(StdError::generic_err("wrong id")),
        }
    }

    pub fn mint_reply(&self, _deps: DepsMut, _env: Env, _reply: Reply) -> StdResult<Response> {
        let res: SubMsgResponse = _reply.result.into_result().map_err(StdError::generic_err)?;

        let mint_event = res
            .events
            .iter()
            .find(|event| {
                event
                    .attributes
                    .iter()
                    .any(|attr| attr.key == "action" && attr.value == "mint")
            })
            .ok_or_else(|| StdError::generic_err("wrong event"))?;

        let minted_token_attr = mint_event
            .attributes
            .iter()
            .cloned()
            .find(|attr| attr.key == "token_id")
            .ok_or_else(|| StdError::generic_err("no token_id"))?;
            
        let temp = TEMP.load(_deps.storage)?;
        TEMP.remove(_deps.storage);

        let funds = format!("{}:{}",temp.funds.denom, temp.funds.amount);

        Ok(Response::new().add_attribute(minted_token_attr.key, minted_token_attr.value).add_attribute("attached_funds", funds))
    }
}
