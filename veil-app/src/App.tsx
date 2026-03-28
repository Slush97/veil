import { useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useAppStore } from './store/appStore';
import { MainLayout } from './components/layout';
import { SetupFlow, RecoveryPhrase, LoadingScreen, Onboarding } from './components/setup';
import { useTauriEvents } from './hooks/useTauriEvents';

function App() {
  const screen = useAppStore((s) => s.ui.screen);
  const theme = useAppStore((s) => s.ui.theme);
  const setScreen = useAppStore((s) => s.setScreen);

  // Subscribe to Tauri backend events
  useTauriEvents();

  // Boot sequence: check if identity exists
  useEffect(() => {
    if (screen !== 'loading') return;

    invoke<boolean>('has_identity')
      .then((exists) => {
        useAppStore.setState({ hasExistingIdentity: exists });
        setScreen('setup');
      })
      .catch(() => {
        // Tauri not available (browser dev mode) — go to setup
        setScreen('setup');
      });
  }, [screen, setScreen]);

  // Hydrate from backend when transitioning to chat
  useEffect(() => {
    if (screen === 'chat') {
      useAppStore.getState().hydrateFromBackend();
    }
  }, [screen]);

  return (
    <div data-theme={theme}>
      {screen === 'loading' && <LoadingScreen />}
      {screen === 'setup' && <SetupFlow />}
      {screen === 'recovery' && <RecoveryPhrase />}
      {screen === 'onboarding' && <Onboarding />}
      {screen === 'chat' && <MainLayout />}
    </div>
  );
}

export default App;
