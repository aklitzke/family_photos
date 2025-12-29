import './App.css'

function App() {
  return (
    <div className="landing-page">
      <header className="hero">
        <div className="hero-content">
          <h1 className="hero-title">Family Photos</h1>
          <p className="hero-subtitle">
            Preserve your precious memories in one beautiful place
          </p>
          <p className="hero-description">
            A simple, elegant way to store, organize, and share your family's most cherished moments.
            Built with modern technology to keep your memories safe and accessible.
          </p>
          <div className="hero-actions">
            <button className="btn btn-primary">Get Started</button>
            <button className="btn btn-secondary">Learn More</button>
          </div>
        </div>
      </header>

      <section className="features">
        <div className="feature">
          <div className="feature-icon">📸</div>
          <h3>Easy Upload</h3>
          <p>Upload your photos with a simple drag and drop interface</p>
        </div>
        <div className="feature">
          <div className="feature-icon">🗂️</div>
          <h3>Smart Organization</h3>
          <p>Automatically organize photos by date, event, or custom albums</p>
        </div>
        <div className="feature">
          <div className="feature-icon">🔒</div>
          <h3>Secure Storage</h3>
          <p>Your memories are safely stored and backed up</p>
        </div>
        <div className="feature">
          <div className="feature-icon">👨‍👩‍👧‍👦</div>
          <h3>Family Sharing</h3>
          <p>Share albums with family members effortlessly</p>
        </div>
      </section>

      <footer className="footer">
        <p>&copy; 2025 Family Photos. Built with Rust & TypeScript.</p>
      </footer>
    </div>
  )
}

export default App
