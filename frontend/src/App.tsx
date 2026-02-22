import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { BrowserRouter, Routes, Route } from 'react-router-dom';
import { Layout } from './components/Layout';
import { DashboardPage } from './features/dashboard/DashboardPage';
import { EventsPage } from './features/events/EventsPage';
import { SessionsPage } from './features/sessions/SessionsPage';
import { PersonasPage } from './features/personas/PersonasPage';
import { FocusPage } from './features/focus/FocusPage';
import { RepoPage } from './features/repos/RepoPage';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 10000, // 10 seconds
      retry: 1,
    },
  },
});

function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <Routes>
          <Route path="/" element={<Layout />}>
            <Route index element={<DashboardPage />} />
            <Route path="events" element={<EventsPage />} />
            <Route path="sessions" element={<SessionsPage />} />
            <Route path="personas" element={<PersonasPage />} />
            <Route path="focus" element={<FocusPage />} />
            <Route path="repos/:remoteUrl" element={<RepoPage />} />
          </Route>
        </Routes>
      </BrowserRouter>
    </QueryClientProvider>
  );
}

export default App;
