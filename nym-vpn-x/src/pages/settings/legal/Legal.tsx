import { open } from '@tauri-apps/api/shell';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { PrivacyPolicyUrl, ToSUrl } from '../../../constants';
import { routes } from '../../../router';
import { MsIcon, PageAnim } from '../../../ui';
import SettingsGroup from '../SettingsGroup';

function Legal() {
  const { t } = useTranslation('settings');
  const navigate = useNavigate();

  return (
    <PageAnim className="h-full flex flex-col mt-2 gap-6">
      <SettingsGroup
        settings={[
          {
            title: t('legal.tos'),
            onClick: async () => open(ToSUrl),
            trailing: <MsIcon icon="arrow_right" />,
          },
          {
            title: t('legal.policy'),
            onClick: async () => open(PrivacyPolicyUrl),
            trailing: <MsIcon icon="arrow_right" />,
          },
        ]}
      />
      <SettingsGroup
        settings={[
          {
            title: t('legal.licenses-rust'),
            onClick: async () => navigate(routes.licensesRust),
            trailing: <MsIcon icon="arrow_right" />,
          },
          {
            title: t('legal.licenses-js'),
            onClick: async () => navigate(routes.licensesJs),
            trailing: <MsIcon icon="arrow_right" />,
          },
        ]}
      />
    </PageAnim>
  );
}

export default Legal;
