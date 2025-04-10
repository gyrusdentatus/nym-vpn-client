import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { motion } from 'motion/react';
import dayjs from 'dayjs';
import { useMainState } from '../../contexts';

function ConnectionTimer() {
  const { tunnelConnectedAt } = useMainState();
  const [connectionTime, setConnectionTime] = useState('00:00:00');
  const { t } = useTranslation('home');

  useEffect(() => {
    if (!tunnelConnectedAt) {
      return;
    }

    const elapsed = dayjs.duration(dayjs().diff(tunnelConnectedAt));
    setConnectionTime(elapsed.format('HH:mm:ss'));

    const interval = setInterval(() => {
      const elapsed = dayjs.duration(dayjs().diff(tunnelConnectedAt));
      setConnectionTime(elapsed.format('HH:mm:ss'));
    }, 500);

    return () => {
      clearInterval(interval);
    };
  }, [tunnelConnectedAt]);

  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.9 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.1, ease: 'easeOut' }}
      className="flex flex-col items-center gap-2 cursor-default select-none"
    >
      <p className="text-base text-iron dark:text-bombay">
        {t('connection-time')}
      </p>
      <p className="text-base text-baltic-sea dark:text-white">
        {connectionTime}
      </p>
    </motion.div>
  );
}

export default ConnectionTimer;
