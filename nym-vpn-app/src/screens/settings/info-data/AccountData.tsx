import { useEffect, useState } from 'react';
import clsx from 'clsx';
import { invoke } from '@tauri-apps/api/core';
import { useTranslation } from 'react-i18next';
import { useMainState } from '../../../contexts';
import { CCache } from '../../../cache';
import { useClipboard } from '../../../hooks';
import { ButtonText } from '../../../ui';

const IdsTimeToLive = 120; // sec

function AccountData() {
  const [accountId, setAccountId] = useState<string | null>(null);
  const [deviceId, setDeviceId] = useState<string | null>(null);
  const { account } = useMainState();
  const { copy } = useClipboard();

  const { t } = useTranslation('settings');

  const getAccountId = async () => {
    const id = await CCache.get<string>('cache-account-id');
    if (id) {
      setAccountId(id);
      return;
    }
    try {
      const id = await invoke<string | null>('get_account_id');
      setAccountId(id);
      CCache.set('cache-account-id', id, IdsTimeToLive);
    } catch (e) {
      console.warn('failed to get account id', e);
      setAccountId(null);
    }
  };

  const getDeviceId = async () => {
    const id = await CCache.get<string>('cache-device-id');
    if (id) {
      setDeviceId(id);
      return;
    }
    try {
      const id = await invoke<string | null>('get_device_id');
      setDeviceId(id);
      await CCache.set('cache-device-id', id, IdsTimeToLive);
    } catch (e) {
      console.warn('failed to get device id', e);
      setDeviceId(null);
    }
  };

  useEffect(() => {
    if (account) {
      getAccountId();
      getDeviceId();
    }
  }, [account]);

  if (!account) {
    return null;
  }

  const truncateId = (id: string) => {
    if (id.length < 16) {
      return id;
    }
    return `${id.slice(0, 8)}…${id.slice(-8)}`;
  };

  return (
    <div className={clsx('mt-3')}>
      {accountId && (
        <div className={clsx('flex flex-row flex-nowrap gap-1')}>
          <p className="text-nowrap">{t('info.account-id')}</p>
          <ButtonText onClick={() => copy(accountId)} truncate>
            {truncateId(accountId)}
          </ButtonText>
        </div>
      )}
      {deviceId && (
        <div className={clsx('flex flex-row flex-nowrap gap-1')}>
          <p className="text-nowrap">{t('info.device-id')}</p>
          <ButtonText onClick={() => copy(deviceId)} truncate>
            {truncateId(deviceId)}
          </ButtonText>
        </div>
      )}
    </div>
  );
}

export default AccountData;
