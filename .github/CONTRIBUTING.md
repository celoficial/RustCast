# Contributing to RustCast

Thank you for your interest in contributing to **RustCast**! We welcome contributions of all kinds, including bug fixes, new features, documentation improvements, and more. This guide outlines the process for contributing.

## Project Structure

```plaintext
dlna-server/
├── src/
│   ├── config/         # Configuration (environment variable parsing and structs)
│   ├── discovery/      # SSDP and UPnP logic for device discovery
│   ├── media/          # Media management (videos, subtitles, transcoding)
│   ├── server/         # HTTP server for XML responses and media streaming
│   ├── utils/          # Generic utilities (logging, etc.)
│   ├── main.rs         # Application entry point
├── .env                # Environment variables
├── .env.example        # Environment variable template
├── Cargo.toml          # Dependencies and metadata
├── LICENSE             # Apache 2.0 license file
```

## Getting Started

1. **Fork the repository**: Create your own fork of the project by clicking the "Fork" button on the GitHub page.
2. **Clone your fork**: Clone your fork locally using:

   ```bash
   git clone https://github.com/<your-username>/RustCast.git
   cd RustCast
   ```

3. **Create a new branch**: Create a branch for your contribution:
   ```bash
   git checkout -b feature/my-awesome-feature
   ```

## How to Contribute

### Bug Reports

- Use the GitHub [Issues](https://github.com/your-org-name/RustCast/issues) page to report bugs.
- Include as much detail as possible:
  - Steps to reproduce
  - Expected behavior
  - Actual behavior
  - Screenshots, if applicable

### Feature Requests

- Submit a feature request via the Issues page or discuss it in the Discussions section.
- Provide clear motivation and use cases for your feature.

### Coding Contributions

1. **Code Standards**:

   - Follow Rust best practices.
   - Ensure code is well-documented and tested.

2. **Testing**:

   - Add tests for new features and bug fixes.
   - Run all tests before submitting your contribution:
     ```bash
     cargo test
     ```

3. **Commit Guidelines**:

   - Use clear and descriptive commit messages.
   - Follow conventional commit standards when possible:
     ```
     feat: Add DLNA media rendering feature
     fix: Resolve crash during SSDP discovery
     ```

4. **Pull Requests**:
   - Push your branch to your fork:
     ```bash
     git push origin feature/my-awesome-feature
     ```
   - Open a pull request against the main repository. Be sure to:
     - Provide a clear description of your changes.
     - Link to related issues, if applicable.

## Communication

If you have any questions or need help, please feel free to:

- Open an issue or discussion on GitHub.
- Contact the maintainers via GitHub.

---

Thank you for contributing to RustCast! Together, we can create an amazing open-source project.
