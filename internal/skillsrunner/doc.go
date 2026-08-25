// Package skillsrunner drives the in-process skills pipeline for symbrain
// sync. It replaces the archived symskills binary: bundles are loaded
// from the skills library root and rendered + installed per harness
// target through internal/skills, so a released symbrain needs no
// external symskills binary on PATH.
package skillsrunner
