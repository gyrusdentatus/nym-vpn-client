import { DialogTitle } from '@headlessui/react';
import { useTranslation } from 'react-i18next';
import { Button, Dialog, Link, MsIcon } from '../../ui';
import { capFirst } from '../../util';
import { ModesDetailsArticle } from '../../constants';

export type Props = {
  isOpen: boolean;
  onClose: () => void;
};

function ModeDetailsDialog({ isOpen, onClose }: Props) {
  const { t } = useTranslation('home');

  return (
    <Dialog
      open={isOpen}
      onClose={onClose}
      className="flex flex-col items-center gap-6"
    >
      <div className="flex flex-col items-center gap-4">
        <MsIcon
          icon="info"
          className="text-3xl text-baltic-sea dark:text-white"
        />
        <DialogTitle
          as="h3"
          className="text-xl text-baltic-sea dark:text-white"
        >
          {t('modes-dialog.title')}
        </DialogTitle>
      </div>
      <div className="flex flex-col gap-2">
        <div className="flex flex-row items-center text-baltic-sea dark:text-white gap-2">
          <MsIcon icon="speed" />
          <h4 className="text-lg">{t('vpn-modes.fast', { ns: 'common' })}</h4>
        </div>
        <p className="text-iron dark:text-bombay md:text-nowrap">
          {t('modes-dialog.fast-description')}
        </p>
      </div>
      <div className="flex flex-col gap-2">
        <div className="flex flex-row items-center text-baltic-sea dark:text-white gap-2">
          <MsIcon icon="visibility_off" />
          <h4 className="text-lg">
            {t('vpn-modes.privacy', { ns: 'common' })}
          </h4>
        </div>
        <p className="text-iron dark:text-bombay md:text-nowrap">
          {t('modes-dialog.privacy-description')}
        </p>
      </div>
      <Link
        text={t('modes-dialog.link')}
        url={ModesDetailsArticle}
        className="mb-1"
        icon
      />
      <Button onClick={onClose} className="mt-2">
        <span className="text-lg text-black dark:text-baltic-sea">
          {capFirst(t('ok', { ns: 'glossary' }))}
        </span>
      </Button>
    </Dialog>
  );
}

export default ModeDetailsDialog;
