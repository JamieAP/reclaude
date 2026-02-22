import { Outlet, NavLink } from 'react-router-dom';

const navItems = [
  { to: '/', label: 'Dashboard' },
  { to: '/events', label: 'Events' },
  { to: '/sessions', label: 'Sessions' },
  { to: '/personas', label: 'Personas' },
  { to: '/focus', label: 'Focus' },
];

export function Layout() {
  return (
    <div className="flex flex-col min-h-screen" style={{ backgroundColor: 'var(--ctp-base)' }}>
      {/* Header with navigation */}
      <header
        className="h-12 flex items-center justify-between px-4 border-b flex-shrink-0"
        style={{
          backgroundColor: 'var(--ctp-mantle)',
          borderColor: 'var(--ctp-surface0)',
        }}
      >
        {/* Logo + Nav */}
        <div className="flex items-center gap-6">
          <h1
            className="text-base font-semibold"
            style={{ color: 'var(--ctp-peach)' }}
          >
            reclaude
          </h1>
          <nav className="flex items-center gap-1">
            {navItems.map((item) => (
              <NavLink
                key={item.to}
                to={item.to}
                end={item.to === '/'}
                className={({ isActive }) =>
                  `px-3 py-1.5 rounded text-sm transition-colors ${isActive ? 'font-medium' : ''}`
                }
                style={({ isActive }) => ({
                  backgroundColor: isActive ? 'var(--ctp-surface0)' : 'transparent',
                  color: isActive ? 'var(--ctp-blue)' : 'var(--ctp-subtext0)',
                })}
              >
                {item.label}
              </NavLink>
            ))}
          </nav>
        </div>

        {/* Right side */}
        <div className="flex items-center gap-3">
          <span
            className="text-xs px-2 py-1 rounded"
            style={{
              backgroundColor: 'var(--ctp-surface0)',
              color: 'var(--ctp-subtext0)'
            }}
          >
            Local
          </span>
        </div>
      </header>

      {/* Page content */}
      <main className="flex-1 p-4 overflow-auto">
        <Outlet />
      </main>
    </div>
  );
}
