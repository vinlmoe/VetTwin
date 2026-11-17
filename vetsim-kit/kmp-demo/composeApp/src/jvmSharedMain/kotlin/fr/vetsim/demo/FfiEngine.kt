package fr.vetsim.demo

import fr.vetsim.ffi.Engine

/** Moteur Rust via les liaisons UniFFI (JNA), commun à Android et au bureau JVM. */
class FfiEngine : VetsimEngine {
    private val e = Engine()
    override fun apiVersion() = e.apiVersion()
    override fun loadModules(json: String) = e.loadModules(json)
    override fun newPatient(optionsJson: String) = e.newPatient(optionsJson)
    override fun addDisease(patientId: String, optionsJson: String) = e.addDisease(patientId, optionsJson)
    override fun state(patientId: String, optionsJson: String) = e.state(patientId, optionsJson)
    override fun signals(patientId: String, optionsJson: String) = e.signals(patientId, optionsJson)
    override fun timecourse(patientId: String, dosesJson: String) = e.timecourse(patientId, dosesJson)
    override fun listModules() = e.listModules()
}

actual fun createEngine(): VetsimEngine = FfiEngine()
