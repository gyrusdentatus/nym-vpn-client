import { BackendError, ErrorKey } from './tauri';

export type TunnelConnected = { connected: Tunnel };
export type TunnelConnecting = {
  connecting: Tunnel | null;
};
export type TunnelDisconnecting = { disconnecting: TunnelAction | null };
export type TunnelStateError = { error: TunnelError };
export type TunnelOffline = {
  offline: { reconnect: boolean };
};
type TunnelState =
  | 'disconnected'
  | TunnelConnected
  | TunnelConnecting
  | TunnelDisconnecting
  | TunnelStateError
  | TunnelOffline;
export type TunnelStateIpc = TunnelState;

export function isTunnelConnected(
  state: TunnelState,
): state is TunnelConnected {
  return (state as TunnelConnected).connected !== undefined;
}

export function isTunnelConnecting(
  state: TunnelState,
): state is TunnelConnecting {
  return (state as TunnelConnecting).connecting !== undefined;
}

export function isTunnelDisconnecting(
  state: TunnelState,
): state is TunnelDisconnecting {
  return (state as TunnelDisconnecting).disconnecting !== undefined;
}

export function isTunnelOffline(state: TunnelState): state is TunnelOffline {
  return (state as TunnelOffline).offline !== undefined;
}

export function isTunnelError(state: TunnelState): state is TunnelStateError {
  return (state as TunnelStateError).error !== undefined;
}

export type Tunnel = {
  entryGwId: string;
  exitGwId: string;
  connectedAt: number | null; // unix timestamp
  data: TunnelData;
};

export type TunnelData = MixnetData | WireguardData;

export function isMixnetData(data: TunnelData): data is MixnetData {
  return (data as MixnetData).nymAddress !== undefined;
}

export function isWireguardData(data: TunnelData): data is WireguardData {
  return (
    (data as WireguardData).entry !== undefined &&
    (data as WireguardData).exit !== undefined
  );
}

export type TunnelError =
  | { key: 'internal'; data: string | null }
  | {
      key: 'dns';
      data: string | null;
    }
  | { key: 'api'; data: string | null }
  | {
      key: 'firewall';
      data: string | null;
    }
  | { key: 'routing'; data: string | null }
  | {
      key: 'same-entry-and-exit-gw';
      data: string | null;
    }
  | { key: 'invalid-entry-gw-country'; data: string | null }
  | {
      key: 'invalid-exit-gw-country';
      data: string | null;
    }
  | { key: 'max-devices-reached'; data: string | null }
  | {
      key: 'bandwidth-exceeded';
      data: string | null;
    }
  | { key: 'subscription-expired'; data: string | null };

export type TunnelStateEvent = {
  state: TunnelState;
  error: BackendError | null;
};

export type TunnelAction = 'error' | 'reconnect' | 'offline';

export type MxAddress = { nymAddress: string; gatewayId: string };

export type MixnetData = {
  nymAddress: MxAddress | null;
  exitIpr: MxAddress | null;
  ipv4: string;
  ipv6: string;
  entryIp: string;
  exitIp: string;
};

export type WireguardData = { entry: WgNode; exit: WgNode };

export type WgNode = {
  endpoint: string;
  publicKey: string;
  privateIpv4: string;
  privateIpv6: string;
};

export type RemainingBandwidth = {
  'remaining-bandwidth': bigint;
};
export type MixnetEvent =
  | 'entry-gw-down'
  | 'exit-gw-down-ipv4'
  | 'exit-gw-down-ipv6'
  | 'exit-gw-routing-error-ipv4'
  | 'exit-gw-routing-error-ipv6'
  | 'connected-ipv4'
  | 'connected-ipv6'
  | 'no-bandwidth'
  | RemainingBandwidth
  | 'sphinx-packet-metrics';

export function isRemainingBandwidth(
  event: MixnetEvent,
): event is RemainingBandwidth {
  return (event as RemainingBandwidth)['remaining-bandwidth'] !== undefined;
}

export type MixnetEventPayload =
  | { event: MixnetEvent }
  | {
      error: ErrorKey;
    };

export function isMixnetEventError(
  payload: MixnetEventPayload,
): payload is { error: ErrorKey } {
  return (payload as { error: ErrorKey }).error !== undefined;
}
