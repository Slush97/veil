import { useEffect } from 'react';
import { useAppStore } from './store/appStore';
import { MainLayout } from './components/layout';
import { SetupFlow, LoadingScreen, Onboarding } from './components/setup';
import { ServerConnect } from './components/setup/ServerConnect';
import { useWebSocketEvents } from './hooks/useWebSocketEvents';
import { getServerUrl, getToken, getMe, connectWs } from './api';

function App() {
  const screen = useAppStore((s) => s.ui.screen);
  const theme = useAppStore((s) => s.ui.theme);
  const setScreen = useAppStore((s) => s.setScreen);

  // Subscribe to WebSocket events
  useWebSocketEvents();

  // Boot sequence: check localStorage for serverUrl+token -> validate with getMe() -> chat or login
  useEffect(() => {
    if (screen !== 'loading') return;

    const serverUrl = getServerUrl();
    const token = getToken();

    if (!serverUrl) {
      setScreen('server-connect');
      return;
    }

    if (!token) {
      setScreen('setup');
      return;
    }

    // Validate token
    getMe()
      .then((user) => {
        useAppStore.setState({
          auth: { token, serverUrl, user },
          identity: {
            masterPeerId: user.id,
            username: user.username,
            displayName: user.display_name,
            bio: user.bio ?? '',
            status: user.status ?? '',
            isSetUp: true,
          },
        });
        connectWs(token);
        setScreen('chat');
      })
      .catch(() => {
        // Token invalid or expired
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
      {screen === 'server-connect' && <ServerConnect />}
      {screen === 'setup' && <SetupFlow />}
      {screen === 'onboarding' && <Onboarding />}
      {screen === 'chat' && <MainLayout />}
    </div>
  );
}

export default App;
