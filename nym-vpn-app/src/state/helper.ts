import i18n from 'i18next';
import {
  DaemonStatus,
  StateDispatch,
  VpndStatus,
  isVpndNonCompat,
  isVpndOk,
} from '../types';
import { Notification } from '../contexts';

export type TauriReq<
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  Req extends (a: never, b?: never) => Promise<any>,
> = {
  name: string;
  request: () => ReturnType<Req>;
  onFulfilled: (value: Awaited<ReturnType<Req>>) => void;
};

// Fires a list of Tauri requests concurrently and handles the results
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export async function fireRequests(requests: TauriReq<any>[]) {
  // eslint-disable-next-line @typescript-eslint/no-unsafe-return
  const promises = await Promise.allSettled(requests.map((r) => r.request()));

  promises.forEach((res, index) => {
    if (res.status === 'rejected') {
      console.warn(
        `command [${requests[index].name}] failed with error:`,
        res.reason,
      );
    }
    if (res.status === 'fulfilled') {
      requests[index].onFulfilled(res.value as never);
    }
  });
}

export function daemonStatusUpdate(
  status: VpndStatus,
  dispatch: StateDispatch,
  push: (notification: Notification) => void,
) {
  dispatch({
    type: 'set-daemon-status',
    status: vpndStatusToState(status),
  });
  let info;
  if (isVpndOk(status) && status.ok) {
    info = status.ok;
  }
  if (isVpndNonCompat(status)) {
    info = status.nonCompat.current;
    push({
      id: 'daemon-no-compat',
      message: i18n.t('daemon-no-compat', {
        ns: 'notifications',
        version: status.nonCompat.current.version,
        required: status.nonCompat.requirement,
      }),
      close: true,
      duration: 6000,
      type: 'warn',
      throttle: 10,
    });
  }
  if (status === 'down') {
    push({
      id: 'daemon-not-connected',
      message: i18n.t('daemon-not-connected', {
        ns: 'notifications',
      }),
      close: true,
      duration: 6000,
      type: 'error',
      throttle: 10,
    });
  }
  if (info) {
    dispatch({ type: 'set-daemon-info', info });
  }
}

function vpndStatusToState(status: VpndStatus): DaemonStatus {
  if (isVpndOk(status)) {
    return 'ok';
  }
  if (isVpndNonCompat(status)) {
    return 'non-compat';
  }
  return 'down';
}
