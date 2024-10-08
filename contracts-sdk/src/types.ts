export type Uint128 = string;
export type Binary = string;
export type Asset = {
  native: Coin;
} | {
  cw20: Cw20Coin;
};
export type Addr = string;
export interface Cw20ReceiveMsg {
  amount: Uint128;
  msg: Binary;
  sender: string;
}
export interface Coin {
  amount: Uint128;
  denom: string;
}
export interface Cw20Coin {
  address: string;
  amount: Uint128;
}
export interface IbcInfo {
  fee?: IbcFee | null;
  memo: string;
  receiver: string;
  recover_address: string;
  source_channel: string;
}
export interface IbcFee {
  ack_fee: Coin[];
  recv_fee: Coin[];
  timeout_fee: Coin[];
}
export interface TransferBackMsg {
  local_channel_id: string;
  memo?: string | null;
  remote_address: string;
  remote_denom: string;
  timeout?: number | null;
}
export interface Route {
  offer_asset: Asset;
  operations: SwapOperation[];
}
export interface SwapOperation {
  denom_in: string;
  denom_out: string;
  interface?: Binary | null;
  pool: string;
}